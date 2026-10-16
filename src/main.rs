#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Context;
use http_selector::app_options::{parse_arguments, test_scan_disabled};
use http_selector::app_paths::AppPaths;
use http_selector::app_title;
use http_selector::default_rule_ui::{RuleDraft, RulesAction, run_rules};
use http_selector::integrity::{IntegrityManager, IntegrityStatus};
use http_selector::management::{self, Event};
use http_selector::model::{DefaultRule, DetectedProfile, Profile};
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
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let options = parse_arguments(&arguments)?;
    let startup = options.startup;
    let mut url = options.url;
    let _instance_guard = match acquire(url.as_ref())? {
        InstanceStatus::Primary(guard) => guard,
        InstanceStatus::Forwarded => return Ok(()),
    };
    let management = management::start(&options.origin)?;

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
        let mut detected = if test_scan_disabled() {
            Vec::new()
        } else {
            profile_scan::scan_all().context("初回プロファイル検出に失敗しました")?
        };
        http_selector::browser::cache_detected_icons(&paths, &mut detected)?;
        integrity.backup_current()?;
        storage.sync_detected(&detected)?;
        storage.mark_initial_scan_completed()?;
        integrity.sign_current()?;
    }
    management.set_ready(true);
    let mut display = !startup || url.is_some();
    loop {
        if display {
            run_session(&paths, &storage, &integrity, url.take(), &management)?;
        }
        let event = management.try_event();
        let event = if event.is_some() {
            event
        } else if startup {
            Some(management.wait_event())
        } else {
            None
        };
        match event {
            Some(Event::Url(incoming)) => {
                url = Some(incoming);
                display = true;
            }
            Some(Event::Show) => {
                display = management.show_requested();
            }
            Some(Event::Stop) | None => break,
        }
    }
    Ok(())
}

fn run_session(
    paths: &AppPaths,
    storage: &Storage,
    integrity: &IntegrityManager,
    mut url: Option<ValidatedUrl>,
    management: &management::Management,
) -> anyhow::Result<()> {
    let no_scan = test_scan_disabled();
    let mut background_scan = (!no_scan).then(|| profile_scan::start_background(paths.clone()));
    let mut escape_hold_duration_tenths = storage.escape_hold_duration_tenths()?;
    let mut open_profile_on_double_click = storage.open_profile_on_double_click()?;
    let mut interruption_enabled = storage.interruption_enabled()?;
    let mut apply_default_rules = url.is_some();
    let mut selected_profile_id = None;

    loop {
        management.set_processing_url(url.is_some());
        if background_scan
            .as_ref()
            .is_some_and(profile_scan::BackgroundProfileScan::is_consumed)
        {
            background_scan = None;
        }
        if apply_default_rules {
            if let Some(url) = url.as_ref() {
                let all_profiles = storage.load_all_profiles()?;
                let rules = storage.load_default_rules()?;
                let target_profile =
                    resolve_profile(&all_profiles, &rules, url, interruption_enabled);
                if let Some(profile) = target_profile {
                    let launched = open_profile(&profile, url, || Ok(true));
                    if let Err(ref error) = launched {
                        show_error(
                            &app_title::main(),
                            &format!(
                                "既定ルールのプロファイルを起動できませんでした。別のプロファイルを選択できます。\n{error:#}"
                            ),
                        );
                    } else if launched?.is_some() {
                        if storage.record_url_history_enabled()? {
                            integrity.backup_current()?;
                            let pattern = http_selector::url_pattern::from_url(url);
                            storage.record_url_pattern(profile.id, &pattern)?;
                            integrity.sign_current()?;
                        }
                        break;
                    } else {
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
            interruption_enabled,
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
                let current_url = selected_url
                    .as_ref()
                    .context("開くURLが指定されていません")?;
                let mut pending_url = None;
                let launched = open_profile(&profile, current_url, || {
                    if storage.association_prompt_suppressed(current_url)? {
                        return Ok(true);
                    }
                    let outcome =
                        http_selector::association_ui::run(current_url, &profile, |rule| {
                            integrity.backup_current()?;
                            storage.save_association(profile.id, rule)?;
                            if let Err(error) = integrity.sign_current() {
                                integrity
                                    .restore_backup()
                                    .context("署名失敗後の設定復元に失敗しました")?;
                                return Err(error);
                            }
                            Ok(())
                        })?;
                    pending_url = outcome.pending_url;
                    Ok(outcome.action == http_selector::association_ui::Action::Open)
                });
                match launched {
                    Ok(Some(true)) => {}
                    Ok(Some(false)) => {
                        selected_profile_id = Some(profile_id);
                        apply_default_rules = pending_url.is_some();
                        url = pending_url.or(selected_url);
                        continue;
                    }
                    Ok(None) => break,
                    Err(error) => {
                        show_error(
                            &app_title::main(),
                            &format!("プロファイルを起動できませんでした。\n{error:#}"),
                        );
                        selected_profile_id = Some(profile_id);
                        apply_default_rules = pending_url.is_some();
                        url = pending_url.or(selected_url);
                        continue;
                    }
                }
                if storage.record_url_history_enabled()? {
                    integrity.backup_current()?;
                    let pattern = http_selector::url_pattern::from_url(current_url);
                    storage.record_url_pattern(profile.id, &pattern)?;
                    integrity.sign_current()?;
                }
                if let Some(incoming) = pending_url {
                    url = Some(incoming);
                    apply_default_rules = true;
                    selected_profile_id = Some(profile_id);
                    continue;
                }
                break;
            }
            SelectorResult::InterruptionChanged {
                enabled,
                url: current_url,
            } => {
                integrity.backup_current()?;
                storage.set_interruption_enabled(enabled)?;
                integrity.sign_current()?;
                interruption_enabled = enabled;
                url = current_url;
                apply_default_rules = true;
            }
            SelectorResult::ConfigureDefaultRule {
                profile_id,
                url: current_url,
            } => {
                let _busy = management::busy_guard();
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
                match scan_profiles(paths) {
                    Ok(detected) => {
                        let changes = apply_detected_profiles(storage, integrity, &detected)?;
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
                apply_detected_profiles(storage, integrity, &detected)?;
            }
            SelectorResult::Settings { url: current_url } => {
                let _busy = management::busy_guard();
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
                    SettingsAction::Rescan => match scan_profiles(paths) {
                        Ok(detected) => {
                            let changes = apply_detected_profiles(storage, integrity, &detected)?;
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

fn open_profile(
    profile: &Profile,
    url: &ValidatedUrl,
    before_launch: impl FnOnce() -> anyhow::Result<bool>,
) -> anyhow::Result<Option<bool>> {
    let _busy = management::busy_guard();
    http_selector::browser::ensure_profile_is_launchable(profile)?;
    let report = http_selector::risk::inspect(url);
    http_selector::risk::guarded(
        &report,
        |report| http_selector::risk_ui::confirm(url, report),
        || {
            if !before_launch()? {
                return Ok(false);
            }
            http_selector::browser::launch(profile, url)?;
            Ok(true)
        },
    )
}

fn resolve_profile(
    profiles: &[Profile],
    rules: &[DefaultRule],
    url: &ValidatedUrl,
    interruption_enabled: bool,
) -> Option<Profile> {
    rules
        .iter()
        .filter(|rule| {
            rule.enabled
                && !http_selector::association::suppresses_prompt(&rule.pattern)
                && http_selector::rules::matches(&rule.pattern, url.original())
        })
        .find_map(|rule| {
            profiles
                .iter()
                .find(|profile| {
                    profile.id == rule.profile_id && profile.available && profile.enabled
                })
                .cloned()
        })
        .or_else(|| {
            (!interruption_enabled)
                .then(|| {
                    profiles
                        .iter()
                        .find(|profile| profile.favorite && profile.available && profile.enabled)
                        .cloned()
                })
                .flatten()
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use http_selector::model::BrowserType;

    #[test]
    fn arguments_preserve_urls_and_validate_management_options() {
        let args = [
            "--startup",
            "--launch-origin",
            "host_manual",
            "HTTPS://example.com/a?secret=1",
        ]
        .map(String::from);
        let options = parse_arguments(&args).unwrap();
        assert!(options.startup);
        assert_eq!(options.origin, "host_manual");
        assert_eq!(
            options.url.unwrap().original(),
            "HTTPS://example.com/a?secret=1"
        );
        assert!(parse_arguments(&["--launch-origin".into()]).is_err());
        assert!(parse_arguments(&["file:///C:/secret".into()]).is_err());
    }

    fn profile(id: i64, favorite: bool, available: bool) -> Profile {
        Profile {
            id,
            browser_type: BrowserType::Edge,
            profile_id: format!("Profile {id}"),
            detected_name: format!("Profile {id}"),
            alias_name: String::new(),
            comment: String::new(),
            sort_order: id,
            favorite,
            enabled: true,
            available,
            icon_path: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn route_prioritizes_rules_then_first_available_favorite_when_interruption_is_off() {
        let url = ValidatedUrl::parse("https://example.com/path").unwrap();
        let profiles = vec![
            profile(1, true, false),
            profile(2, true, true),
            profile(3, false, true),
        ];
        let rule = DefaultRule {
            id: 1,
            profile_id: 3,
            pattern: "https://example.com/*".into(),
            enabled: true,
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
        };
        assert_eq!(
            resolve_profile(&profiles, std::slice::from_ref(&rule), &url, false)
                .unwrap()
                .id,
            3
        );
        assert_eq!(resolve_profile(&profiles, &[], &url, false).unwrap().id, 2);
        assert!(resolve_profile(&profiles, &[], &url, true).is_none());
        assert!(resolve_profile(&profiles[..1], &[], &url, false).is_none());
    }

    #[test]
    fn question_suppression_never_routes_and_structured_rules_keep_legacy_order() {
        use http_selector::association::{StoredRule, UrlParts};
        let url = ValidatedUrl::parse("https://apps.powerapps.com/play?tenantId=123").unwrap();
        let profiles = vec![profile(1, false, true), profile(2, false, true)];
        let parts = UrlParts::new(&url);
        let condition = parts.condition(&parts.defaults()).unwrap();
        let skip = DefaultRule {
            id: 1,
            profile_id: 1,
            pattern: StoredRule::new(condition.clone(), true).encode().unwrap(),
            enabled: true,
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
        };
        assert!(resolve_profile(&profiles, std::slice::from_ref(&skip), &url, true).is_none());
        let route = DefaultRule {
            id: 2,
            profile_id: 2,
            pattern: StoredRule::new(condition, false).encode().unwrap(),
            ..skip.clone()
        };
        let legacy = DefaultRule {
            id: 3,
            profile_id: 1,
            pattern: "https://apps.powerapps.com/*".into(),
            ..skip.clone()
        };
        assert_eq!(
            resolve_profile(
                &profiles,
                &[skip, legacy.clone(), route.clone()],
                &url,
                true
            )
            .unwrap()
            .id,
            1
        );
        assert_eq!(
            resolve_profile(&profiles, &[route, legacy], &url, true)
                .unwrap()
                .id,
            2
        );
    }
}
