#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Context;
use http_selector::app_paths::AppPaths;
use http_selector::app_title;
use http_selector::default_rule_ui::{RuleDraft, RulesAction, run_rules};
use http_selector::integrity::{IntegrityManager, IntegrityStatus};
use http_selector::model::DetectedProfile;
use http_selector::profile_scan;
use http_selector::settings_ui::{SettingsAction, run_settings};
use http_selector::single_instance::{InstanceStatus, acquire};
use http_selector::storage::Storage;
use http_selector::ui::{
    IntegrityRecoveryAction, SelectorResult, ask_integrity_recovery,
    run_selector_with_selection_and_scan as run_selector, show_error, show_information,
};
use http_selector::url_input::ValidatedUrl;

fn main() {
    if let Err(error) = run() {
        show_error(&app_title::main(), &format!("{error:#}"));
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let argument = std::env::args().nth(1);
    let mut url = argument
        .as_deref()
        .filter(|value| !value.starts_with("--"))
        .map(ValidatedUrl::parse)
        .transpose()?;
    let _instance_guard = match acquire(url.as_ref())? {
        InstanceStatus::Primary(guard) => guard,
        InstanceStatus::Forwarded => return Ok(()),
    };

    let paths = AppPaths::for_current_user()?;
    paths.ensure_data_dir()?;
    let integrity = IntegrityManager::new(&paths);
    match integrity.verify_current()? {
        IntegrityStatus::Missing => {}
        IntegrityStatus::Valid => {}
        IntegrityStatus::Invalid => {
            let has_backup = integrity.verify_backup()? == IntegrityStatus::Valid;
            match ask_integrity_recovery(has_backup) {
                IntegrityRecoveryAction::Restore => integrity.restore_backup()?,
                IntegrityRecoveryAction::Initialize => integrity.quarantine_current()?,
                IntegrityRecoveryAction::Exit => return Ok(()),
            }
        }
    }
    let storage = Storage::new(&paths.database);
    if integrity.verify_current()? == IntegrityStatus::Valid && storage.schema_needs_migration()? {
        integrity.backup_current()?;
    }
    storage.initialize()?;
    // 既存DBへのテーブル追加を含め、スキーマ確定後の内容を署名する。
    integrity.sign_current()?;
    let initial_scan_required = !storage.initial_scan_completed()?;
    if initial_scan_required {
        let mut detected =
            profile_scan::scan_all().context("初回プロファイル検出に失敗しました")?;
        http_selector::browser::cache_detected_icons(&paths, &mut detected)?;
        integrity.backup_current()?;
        storage.sync_detected(&detected)?;
        storage.mark_initial_scan_completed()?;
        integrity.sign_current()?;
    }
    let mut background_scan =
        (!initial_scan_required).then(|| profile_scan::start_background(paths.clone()));
    let mut escape_hold_duration_tenths = storage.escape_hold_duration_tenths()?;
    let mut open_profile_on_double_click = storage.open_profile_on_double_click()?;
    let mut apply_default_rules = url.is_some();
    let mut selected_profile_id = None;

    loop {
        if background_scan
            .as_ref()
            .is_some_and(profile_scan::BackgroundProfileScan::is_consumed)
        {
            background_scan = None;
        }
        if apply_default_rules {
            if let Some(url) = url.as_ref() {
                let all_profiles = storage.load_all_profiles()?;
                let matching_profile = storage
                    .load_default_rules()?
                    .into_iter()
                    .filter(|rule| {
                        rule.enabled && http_selector::rules::matches(&rule.pattern, url.original())
                    })
                    .find_map(|rule| {
                        all_profiles
                            .iter()
                            .find(|profile| {
                                profile.id == rule.profile_id
                                    && profile.available
                                    && profile.enabled
                            })
                            .cloned()
                    });
                if let Some(profile) = matching_profile {
                    let launched = http_selector::browser::ensure_profile_is_launchable(&profile)
                        .and_then(|()| http_selector::browser::launch(&profile, url));
                    if let Err(error) = launched {
                        show_error(
                            &app_title::main(),
                            &format!(
                                "既定ルールのプロファイルを起動できませんでした。別のプロファイルを選択できます。\n{error:#}"
                            ),
                        );
                    } else {
                        if storage.record_url_history_enabled()? {
                            integrity.backup_current()?;
                            let pattern = http_selector::url_pattern::from_url(url);
                            storage.record_url_pattern(profile.id, &pattern)?;
                            integrity.sign_current()?;
                        }
                        break;
                    }
                }
            }
        }
        apply_default_rules = false;
        let profiles = storage.load_all_profiles()?;
        match run_selector(
            url.clone(),
            profiles,
            escape_hold_duration_tenths,
            open_profile_on_double_click,
            selected_profile_id,
            background_scan.clone(),
        )? {
            SelectorResult::UrlReceived(incoming) => {
                url = Some(incoming);
                apply_default_rules = true;
            }
            SelectorResult::Profile {
                profile_id,
                url: selected_url,
            } => {
                let profile = storage
                    .load_all_profiles()?
                    .into_iter()
                    .find(|profile| profile.id == profile_id)
                    .context("選択したプロファイルが見つかりません")?;
                let url = selected_url
                    .as_ref()
                    .context("開くURLが指定されていません")?;
                http_selector::browser::ensure_profile_is_launchable(&profile)?;
                http_selector::browser::launch(&profile, url)?;
                if storage.record_url_history_enabled()? {
                    integrity.backup_current()?;
                    let pattern = http_selector::url_pattern::from_url(url);
                    storage.record_url_pattern(profile.id, &pattern)?;
                    integrity.sign_current()?;
                }
                break;
            }
            SelectorResult::ConfigureDefaultRule {
                profile_id,
                url: current_url,
            } => {
                url = Some(current_url.clone());
                let outcome = run_rules(
                    storage.load_all_profiles()?,
                    storage.load_default_rules()?,
                    Some(RuleDraft {
                        profile_id,
                        url: current_url,
                    }),
                )?;
                if outcome.action == RulesAction::Saved {
                    integrity.backup_current()?;
                    storage.replace_default_rules(&outcome.rules)?;
                    integrity.sign_current()?;
                }
                if let Some(pending_url) = outcome.pending_url {
                    url = Some(pending_url);
                    apply_default_rules = true;
                }
            }
            SelectorResult::FavoriteChanged {
                profile_id,
                favorite,
                url: current_url,
            } => {
                let mut profile = storage
                    .load_all_profiles()?
                    .into_iter()
                    .find(|profile| profile.id == profile_id)
                    .context("お気に入りを変更するプロファイルが見つかりません")?;
                if favorite && !profile.favorite {
                    let favorite_count = storage
                        .load_all_profiles()?
                        .into_iter()
                        .filter(|profile| profile.favorite)
                        .count();
                    if favorite_count >= 5 {
                        show_information(
                            &app_title::main(),
                            "お気に入りに登録できるプロファイルは5件までです。",
                        );
                    } else {
                        integrity.backup_current()?;
                        profile.favorite = true;
                        storage.save_profile(&profile)?;
                        integrity.sign_current()?;
                    }
                } else if !favorite && profile.favorite {
                    integrity.backup_current()?;
                    profile.favorite = false;
                    storage.save_profile(&profile)?;
                    integrity.sign_current()?;
                }
                url = current_url;
                selected_profile_id = Some(profile_id);
            }
            SelectorResult::RefreshRequested {
                url: current_url,
                selected_profile_id: current_selected_profile_id,
            } => {
                url = current_url;
                selected_profile_id = current_selected_profile_id;
                match scan_profiles(&paths) {
                    Ok(detected) => {
                        let changes = apply_detected_profiles(&storage, &integrity, &detected)?;
                        if changes.changed() {
                            show_information(&app_title::main(), &changes.message());
                        }
                    }
                    Err(error) => show_error(
                        &app_title::main(),
                        &format!("プロファイルの更新に失敗しました。\n{error:#}"),
                    ),
                }
            }
            SelectorResult::ProfilesDetected {
                detected,
                url: current_url,
                selected_profile_id: current_selected_profile_id,
            } => {
                background_scan = None;
                url = current_url;
                selected_profile_id = current_selected_profile_id;
                apply_detected_profiles(&storage, &integrity, &detected)?;
            }
            SelectorResult::Settings { url: current_url } => {
                url = current_url;
                let outcome = run_settings(
                    storage.load_all_profiles()?,
                    storage.record_url_history_enabled()?,
                    escape_hold_duration_tenths,
                    open_profile_on_double_click,
                )?;
                if outcome.pending_url.is_some() {
                    url = outcome.pending_url;
                    apply_default_rules = true;
                }
                // 再スキャンでも編集中の表示名・メモ・並び順を先に保存する。
                if matches!(
                    outcome.action,
                    SettingsAction::Saved | SettingsAction::Rescan | SettingsAction::RuleList
                ) {
                    integrity.backup_current()?;
                    storage.save_preferences(
                        &outcome.profiles,
                        outcome.record_url_history,
                        outcome.escape_hold_duration_tenths,
                        outcome.open_profile_on_double_click,
                    )?;
                    escape_hold_duration_tenths = outcome.escape_hold_duration_tenths;
                    open_profile_on_double_click = outcome.open_profile_on_double_click;
                    integrity.sign_current()?;
                }
                match outcome.action {
                    SettingsAction::Rescan => match scan_profiles(&paths) {
                        Ok(detected) => {
                            let changes = apply_detected_profiles(&storage, &integrity, &detected)?;
                            if changes.changed() {
                                show_information(&app_title::main(), &changes.message());
                            }
                        }
                        Err(error) => show_error(
                            &app_title::main(),
                            &format!("プロファイル再スキャンに失敗しました。\n{error:#}"),
                        ),
                    },
                    SettingsAction::RuleList => {
                        let rules_outcome = run_rules(
                            storage.load_all_profiles()?,
                            storage.load_default_rules()?,
                            None,
                        )?;
                        if rules_outcome.action == RulesAction::Saved {
                            integrity.backup_current()?;
                            storage.replace_default_rules(&rules_outcome.rules)?;
                            integrity.sign_current()?;
                        }
                        if let Some(pending_url) = rules_outcome.pending_url {
                            url = Some(pending_url);
                            apply_default_rules = true;
                        }
                    }
                    SettingsAction::Saved | SettingsAction::Cancelled => {}
                }
            }
            SelectorResult::Cancelled => break,
        }
    }
    Ok(())
}

fn scan_profiles(paths: &AppPaths) -> anyhow::Result<Vec<DetectedProfile>> {
    let mut detected = profile_scan::scan_all().context("プロファイル検出に失敗しました")?;
    http_selector::browser::cache_detected_icons(paths, &mut detected)?;
    Ok(detected)
}

fn apply_detected_profiles(
    storage: &Storage,
    integrity: &IntegrityManager,
    detected: &[DetectedProfile],
) -> anyhow::Result<profile_scan::ProfileChanges> {
    let changes = profile_scan::changes(&storage.load_all_profiles()?, detected);
    if changes.changed() {
        integrity.backup_current()?;
        storage.sync_detected(detected)?;
        storage.mark_initial_scan_completed()?;
        integrity.sign_current()?;
    }
    Ok(changes)
}
