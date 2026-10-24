#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Context;
use http_selector::app_options::{parse_arguments, test_scan_disabled};
use http_selector::app_paths::AppPaths;
use http_selector::default_rule_ui::{RuleDraft, RulesAction, run_rules};
use http_selector::integrity::{IntegrityManager, IntegrityStatus};
use http_selector::management::{self, Event, Management};
use http_selector::model::{DefaultRule, DetectedProfile, MAX_FAVORITE_PROFILES, Profile};
use http_selector::profile_scan::{self, BackgroundProfileScan, ProfileChanges};
use http_selector::settings_ui::{
    SettingsAction, SettingsPreferences, run_settings_live as run_settings,
};
use http_selector::single_instance::{InstanceStatus, acquire};
use http_selector::storage::Storage;
use http_selector::ui::{
    IntegrityRecoveryAction, SelectorResult, ask_integrity_recovery,
    run_selector_with_selection_and_scan as run_selector, show_error, show_information,
};
use http_selector::url_input::ValidatedUrl;
use http_selector::{
    app_title, association, association_ui, browser, nested_url, risk, risk_ui, url_pattern,
};

fn main() {
    if let Err(error) = run() {
        show_error(&app_title::main(), &format!("{error:#}"));
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_SHIFT};
    let force_confirmation = unsafe { GetAsyncKeyState(VK_SHIFT as i32) } < 0;
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let options = parse_arguments(&arguments)?;
    let startup = options.startup;
    let mut url = options
        .url
        .map(|url| url.with_forced_confirmation(force_confirmation));
    let _instance_guard = match acquire(url.as_ref())? {
        InstanceStatus::Primary(guard) => guard,
        InstanceStatus::Forwarded => return Ok(()),
    };
    let management = management::start(&options.origin)?;

    let paths = AppPaths::for_current_user()?;
    paths.ensure_data_dir()?;
    let integrity = IntegrityManager::new(&paths);
    let Some(storage) = open_storage(&paths, &integrity)? else {
        return Ok(());
    };
    management.set_ready(true);
    let mut display = !startup || url.is_some();
    loop {
        if display {
            Session::new(&paths, &storage, &integrity, &management, url.take())?.run()?;
        }
        // 管理用の常駐待機では次の要求を待ち、通常起動では受付済みの要求だけを処理する。
        match management
            .try_event()
            .or_else(|| startup.then(|| management.wait_event()))
        {
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

/// 署名を確認してDBを開き、スキーマ更新と初回のプロファイル検出を済ませる。
/// 破損時の確認で利用者が終了を選んだ場合は`None`を返す。
fn open_storage(paths: &AppPaths, integrity: &IntegrityManager) -> anyhow::Result<Option<Storage>> {
    if integrity.verify_current()? == IntegrityStatus::Invalid {
        let has_backup = integrity.verify_backup()? == IntegrityStatus::Valid;
        match ask_integrity_recovery(has_backup) {
            IntegrityRecoveryAction::Restore => integrity.restore_backup()?,
            IntegrityRecoveryAction::Initialize => integrity.quarantine_current()?,
            IntegrityRecoveryAction::Exit => return Ok(None),
        }
    }
    let storage = Storage::new(&paths.database);
    if integrity.verify_current()? == IntegrityStatus::Valid && storage.schema_needs_migration()? {
        integrity.backup_current()?;
    }
    storage.initialize()?;
    // 既存DBへのテーブル追加を含め、スキーマ確定後の内容を署名する。
    integrity.sign_current()?;
    if !storage.initial_scan_completed()? {
        let mut detected = if test_scan_disabled() {
            Vec::new()
        } else {
            profile_scan::scan_all().context("初回プロファイル検出に失敗しました")?
        };
        browser::cache_detected_icons(paths, &mut detected)?;
        integrity.update(|| {
            storage.sync_detected(&detected)?;
            storage.mark_initial_scan_completed()
        })?;
    }
    Ok(Some(storage))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Flow {
    /// 選択画面を表示し直す。
    Continue,
    /// URLを処理し終えたか、利用者が閉じた。
    Finish,
}

/// 1件のURL（またはURLなしの表示要求）について、選択画面と付随する画面の状態を保持する。
struct Session<'a> {
    paths: &'a AppPaths,
    storage: &'a Storage,
    integrity: &'a IntegrityManager<'a>,
    management: &'a Management,
    url: Option<ValidatedUrl>,
    /// 新しく受け取ったURLだけを既定ルールで自動的に開く。
    apply_default_rules: bool,
    selected_profile_id: Option<i64>,
    background_scan: Option<BackgroundProfileScan>,
    escape_hold_duration_tenths: u32,
    open_profile_on_double_click: bool,
    interruption_enabled: bool,
}

impl<'a> Session<'a> {
    fn new(
        paths: &'a AppPaths,
        storage: &'a Storage,
        integrity: &'a IntegrityManager<'a>,
        management: &'a Management,
        url: Option<ValidatedUrl>,
    ) -> anyhow::Result<Self> {
        let background_scan =
            (!test_scan_disabled()).then(|| profile_scan::start_background(paths.clone()));
        Ok(Self {
            paths,
            storage,
            integrity,
            management,
            apply_default_rules: url.is_some(),
            url,
            selected_profile_id: None,
            background_scan,
            escape_hold_duration_tenths: storage.escape_hold_duration_tenths()?,
            open_profile_on_double_click: storage.open_profile_on_double_click()?,
            interruption_enabled: storage.interruption_enabled()?,
        })
    }

    fn run(mut self) -> anyhow::Result<()> {
        loop {
            self.management.set_processing_url(self.url.is_some());
            if self
                .background_scan
                .as_ref()
                .is_some_and(BackgroundProfileScan::is_consumed)
            {
                self.background_scan = None;
            }
            if self.apply_default_rules && self.open_with_default_rule()? == Flow::Finish {
                return Ok(());
            }
            self.apply_default_rules = false;
            let result = run_selector(
                self.url.clone(),
                self.storage.load_all_profiles()?,
                self.escape_hold_duration_tenths,
                self.open_profile_on_double_click,
                self.selected_profile_id,
                self.background_scan.clone(),
                self.interruption_enabled,
            )?;
            if self.handle(result)? == Flow::Finish {
                return Ok(());
            }
        }
    }

    fn handle(&mut self, result: SelectorResult) -> anyhow::Result<Flow> {
        match result {
            SelectorResult::UrlReceived(incoming) => self.receive(incoming),
            SelectorResult::Profile { profile_id, url } => {
                return self.open_selected(profile_id, url);
            }
            SelectorResult::InterruptionChanged { enabled, url } => {
                self.integrity
                    .update(|| self.storage.set_interruption_enabled(enabled))?;
                self.interruption_enabled = enabled;
                self.url = url;
                self.apply_default_rules = true;
            }
            SelectorResult::ConfigureDefaultRule { profile_id, url } => {
                self.url = Some(url.clone());
                self.edit_rules(Some(RuleDraft { profile_id, url }))?;
            }
            SelectorResult::RuleList { url } => {
                self.url = url;
                self.edit_rules(None)?;
            }
            SelectorResult::FavoriteChanged {
                profile_id,
                favorite,
                url,
            } => {
                self.change_favorite(profile_id, favorite)?;
                self.url = url;
                self.selected_profile_id = Some(profile_id);
            }
            SelectorResult::RefreshRequested {
                url,
                selected_profile_id,
            } => {
                self.url = url;
                self.selected_profile_id = selected_profile_id;
                self.rescan("プロファイルの更新に失敗しました。")?;
            }
            SelectorResult::ProfilesDetected {
                detected,
                url,
                selected_profile_id,
            } => {
                self.background_scan = None;
                self.url = url;
                self.selected_profile_id = selected_profile_id;
                apply_detected_profiles(self.storage, self.integrity, &detected)?;
            }
            SelectorResult::Settings { url } => {
                self.url = url;
                self.open_settings()?;
            }
            SelectorResult::Cancelled => return Ok(Flow::Finish),
        }
        Ok(Flow::Continue)
    }

    /// 後着URLを処理対象にし、既定ルールの評価からやり直す。
    fn receive(&mut self, url: ValidatedUrl) {
        self.url = Some(url);
        self.apply_default_rules = true;
    }

    /// 既定ルール（割り込みオフ時はお気に入り）に一致すれば、選択画面を出さずに開く。
    fn open_with_default_rule(&mut self) -> anyhow::Result<Flow> {
        let Some(url) = self.url.as_ref() else {
            return Ok(Flow::Continue);
        };
        let Some(profile) = resolve_profile(
            &self.storage.load_all_profiles()?,
            &self.storage.load_default_rules()?,
            url,
            self.interruption_enabled,
        ) else {
            return Ok(Flow::Continue);
        };
        match open_profile(&profile, url, self.storage, self.integrity, || Ok(true)) {
            Ok(Some(_)) => {
                self.record_history(profile.id, url)?;
                Ok(Flow::Finish)
            }
            Ok(None) => Ok(Flow::Finish),
            Err(error) => {
                show_error(
                    &app_title::main(),
                    &format!(
                        "既定ルールのプロファイルを起動できませんでした。別のプロファイルを選択できます。\n{error:#}"
                    ),
                );
                Ok(Flow::Continue)
            }
        }
    }

    /// 選択されたプロファイルで開く。連携フォームで戻った場合と起動失敗時は選択画面へ戻る。
    fn open_selected(
        &mut self,
        profile_id: i64,
        selected_url: Option<ValidatedUrl>,
    ) -> anyhow::Result<Flow> {
        let (storage, integrity) = (self.storage, self.integrity);
        let profile = storage
            .find_profile(profile_id)?
            .context("選択したプロファイルが見つかりません")?;
        let url = selected_url
            .as_ref()
            .context("開くURLが指定されていません")?;
        let mut replacements = replacement_candidates(storage, url)?;
        let mut pending_url = None;
        let launched = open_profile(&profile, url, storage, integrity, || {
            confirm_association(
                storage,
                integrity,
                &profile,
                url,
                &mut replacements,
                &mut pending_url,
            )
        });
        let opened = match launched {
            Ok(Some(opened)) => opened,
            Ok(None) => return Ok(Flow::Finish),
            Err(error) => {
                show_error(
                    &app_title::main(),
                    &format!("プロファイルを起動できませんでした。\n{error:#}"),
                );
                false
            }
        };
        if !opened {
            self.selected_profile_id = Some(profile_id);
            self.apply_default_rules = pending_url.is_some();
            self.url = pending_url.or(selected_url);
            return Ok(Flow::Continue);
        }
        self.record_history(profile.id, url)?;
        let Some(incoming) = pending_url else {
            return Ok(Flow::Finish);
        };
        self.selected_profile_id = Some(profile_id);
        self.receive(incoming);
        Ok(Flow::Continue)
    }

    /// 利用者が有効にしている場合だけ、伏せ字化したURLパターンを記録する。
    fn record_history(&self, profile_id: i64, url: &ValidatedUrl) -> anyhow::Result<()> {
        if self.storage.record_url_history_enabled()? {
            let pattern = url_pattern::from_url(url);
            self.integrity
                .update(|| self.storage.record_url_pattern(profile_id, &pattern))?;
        }
        Ok(())
    }

    fn edit_rules(&mut self, draft: Option<RuleDraft>) -> anyhow::Result<()> {
        let _busy = management::busy_guard();
        let outcome = run_rules(
            self.storage.load_all_profiles()?,
            self.storage.load_default_rules()?,
            draft,
        )?;
        if outcome.action == RulesAction::Saved {
            self.integrity
                .update(|| self.storage.replace_default_rules(&outcome.rules))?;
        }
        if let Some(pending) = outcome.pending_url {
            self.receive(pending);
        }
        Ok(())
    }

    fn change_favorite(&self, profile_id: i64, favorite: bool) -> anyhow::Result<()> {
        let profiles = self.storage.load_all_profiles()?;
        let mut profile = profiles
            .iter()
            .find(|profile| profile.id == profile_id)
            .cloned()
            .context("お気に入りを変更するプロファイルが見つかりません")?;
        if profile.favorite == favorite {
            return Ok(());
        }
        if favorite
            && profiles.iter().filter(|profile| profile.favorite).count() >= MAX_FAVORITE_PROFILES
        {
            show_information(
                &app_title::main(),
                "お気に入りに登録できるプロファイルは5件までです。",
            );
            return Ok(());
        }
        profile.favorite = favorite;
        self.integrity
            .update(|| self.storage.save_profile(&profile))
    }

    /// 検出の失敗は画面を維持して通知し、保存の失敗だけを呼出元へ返す。
    fn rescan(&self, failure_message: &str) -> anyhow::Result<()> {
        match scan_profiles(self.paths) {
            Ok(detected) => {
                let changes = apply_detected_profiles(self.storage, self.integrity, &detected)?;
                if changes.changed() {
                    show_information(&app_title::main(), &changes.message());
                }
            }
            Err(error) => show_error(&app_title::main(), &format!("{failure_message}\n{error:#}")),
        }
        Ok(())
    }

    fn open_settings(&mut self) -> anyhow::Result<()> {
        let _busy = management::busy_guard();
        let (storage, integrity) = (self.storage, self.integrity);
        let mut save = |preferences: &SettingsPreferences| {
            integrity.update(|| {
                storage.save_preferences_with_quad9(
                    &preferences.profiles,
                    preferences.record_url_history,
                    preferences.escape_hold_duration_tenths,
                    preferences.open_profile_on_double_click,
                    preferences.quad9_enabled,
                )
            })
        };
        let mut test = |profile: &Profile| browser::launch_test(profile);
        let outcome = run_settings(
            storage.load_all_profiles()?,
            storage.record_url_history_enabled()?,
            self.escape_hold_duration_tenths,
            self.open_profile_on_double_click,
            storage.quad9_enabled()?,
            &mut save,
            &mut test,
        )?;
        if let Some(pending) = outcome.pending_url {
            self.receive(pending);
        }
        self.escape_hold_duration_tenths = storage.escape_hold_duration_tenths()?;
        self.open_profile_on_double_click = storage.open_profile_on_double_click()?;
        match outcome.action {
            SettingsAction::ClearDomainApprovals => {
                integrity.update(|| storage.set_approved_domains(&[]))?;
            }
            SettingsAction::Rescan => self.rescan("プロファイル再スキャンに失敗しました。")?,
            SettingsAction::Closed => {}
        }
        Ok(())
    }
}

/// Shift指定で確認するURLだけ、一致する既存ルールを置換候補にする。
fn replacement_candidates(
    storage: &Storage,
    url: &ValidatedUrl,
) -> anyhow::Result<Vec<DefaultRule>> {
    if !url.force_confirmation() {
        return Ok(vec![]);
    }
    Ok(storage
        .load_default_rules()?
        .into_iter()
        .filter(|rule| rule.enabled && http_selector::rules::matches(&rule.pattern, url.original()))
        .collect())
}

/// 連携フォームを表示し、開くが選ばれた場合だけ`true`を返す。
/// 保存は署名付きで即時に行い、フォーム表示中に届いたURLは`pending_url`へ残す。
fn confirm_association(
    storage: &Storage,
    integrity: &IntegrityManager,
    profile: &Profile,
    url: &ValidatedUrl,
    replacements: &mut [DefaultRule],
    pending_url: &mut Option<ValidatedUrl>,
) -> anyhow::Result<bool> {
    if !url.force_confirmation() && storage.association_prompt_suppressed(url)? {
        return Ok(true);
    }
    let preset = (replacements.len() == 1).then(|| replacements[0].clone());
    let outcome = association_ui::run_with_existing(url, profile, preset.as_ref(), |rule| {
        anyhow::ensure!(
            replacements.len() <= 1,
            "複数の登録ルールが一致します。誤置換を防ぐため既定ルール一覧で対象を確認してください"
        );
        integrity.update(|| match replacements.first() {
            Some(existing) => {
                storage.replace_association_target(existing, profile.id, rule.suppress_prompt)
            }
            None => storage.save_association(profile.id, rule),
        })?;
        // 続けて保存しても置換対象の照合が古いスナップショットにならないよう読み直す。
        if let Some(existing) = replacements.first_mut() {
            *existing = storage
                .load_default_rules()?
                .into_iter()
                .find(|entry| entry.id == existing.id)
                .context("保存後のルールを取得できません")?;
        }
        Ok(())
    })?;
    *pending_url = outcome.pending_url;
    Ok(outcome.action == association_ui::Action::Open)
}

fn open_profile(
    profile: &Profile,
    url: &ValidatedUrl,
    storage: &Storage,
    integrity: &IntegrityManager,
    before_launch: impl FnOnce() -> anyhow::Result<bool>,
) -> anyhow::Result<Option<bool>> {
    let _busy = management::busy_guard();
    browser::ensure_profile_is_launchable(profile)?;
    let report = risk::inspect_approved_with_quad9(
        url,
        &storage.approved_domains()?,
        storage.quad9_enabled()?,
    );
    let decision = if report.requires_confirmation() {
        risk_ui::confirm_with_approval(url, &report)
    } else {
        risk_ui::Decision {
            accepted: true,
            approve_domains: false,
        }
    };
    risk::guarded(
        &report,
        |_| decision.accepted,
        || {
            if decision.approve_domains {
                let mut approved = storage.approved_domains()?;
                approved.extend(report.approvable_domains.iter().cloned());
                integrity.update(|| storage.set_approved_domains(&approved))?;
            }
            if !before_launch()? {
                return Ok(false);
            }
            browser::launch(profile, url)?;
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
    if url.force_confirmation() {
        return None;
    }
    let routable =
        |rule: &&DefaultRule| rule.enabled && !association::suppresses_prompt(&rule.pattern);
    let target = |rule: &DefaultRule| {
        profiles
            .iter()
            .find(|profile| profile.id == rule.profile_id && profile.is_selectable())
    };
    if nested_url::is_teams_link(url) {
        // 内側URLの条件を固定したルールだけで振り分け、登録先が競合すれば手動選択にする。
        let mut matches = rules
            .iter()
            .filter(routable)
            .filter(|rule| {
                association::StoredRule::decode(&rule.pattern)
                    .ok()
                    .flatten()
                    .is_some_and(|rule| rule.matches_teams_target(url))
            })
            .filter_map(target);
        let profile = matches.next()?;
        return matches
            .all(|other| other.id == profile.id)
            .then(|| profile.clone());
    }
    rules
        .iter()
        .filter(routable)
        .filter(|rule| http_selector::rules::matches(&rule.pattern, url.original()))
        .find_map(target)
        .or_else(|| {
            // 割り込みオフでは、ルールに一致しないURLを先頭の利用可能なお気に入りで開く。
            if interruption_enabled {
                None
            } else {
                profiles
                    .iter()
                    .find(|profile| profile.favorite && profile.is_selectable())
            }
        })
        .cloned()
}

fn scan_profiles(paths: &AppPaths) -> anyhow::Result<Vec<DetectedProfile>> {
    let mut detected = profile_scan::scan_all().context("プロファイル検出に失敗しました")?;
    browser::cache_detected_icons(paths, &mut detected)?;
    Ok(detected)
}

fn apply_detected_profiles(
    storage: &Storage,
    integrity: &IntegrityManager,
    detected: &[DetectedProfile],
) -> anyhow::Result<ProfileChanges> {
    let changes = profile_scan::changes(&storage.load_all_profiles()?, detected);
    if changes.changed() {
        integrity.update(|| {
            storage.sync_detected(detected)?;
            storage.mark_initial_scan_completed()
        })?;
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

    #[test]
    fn forced_confirmation_bypasses_rules_and_favorites_without_changing_url() {
        let profiles = [profile(1, true, true)];
        let original = ValidatedUrl::parse("https://example.com/a").unwrap();
        let rule = DefaultRule {
            id: 1,
            profile_id: 1,
            pattern: "https://example.com/*".into(),
            enabled: true,
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
        };
        let forced = original.clone().with_forced_confirmation(true);
        for enabled in [true, false] {
            assert!(
                resolve_profile(&profiles, std::slice::from_ref(&rule), &forced, enabled).is_none()
            );
            assert!(resolve_profile(&profiles, &[], &forced, enabled).is_none());
        }
        assert_eq!(forced.original(), original.original());
        assert!(resolve_profile(&profiles, &[rule], &original, true).is_some());
    }

    #[test]
    fn teams_routes_stable_path_without_cid_and_keeps_conflicts_manual() {
        use http_selector::association::{StoredRule, UrlParts};
        let wrapper = |person: &str, query: &str| {
            let mut url = url::Url::parse(http_selector::nested_url::TEAMS_WRAPPER_URL).unwrap();
            url.query_pairs_mut().append_pair(
                "url",
                &format!(
                    "https://tenant-my.sharepoint.com/:f:/g/personal/{person}/document{query}"
                ),
            );
            ValidatedUrl::parse(url.to_string()).unwrap()
        };
        let initial = wrapper("person_example_com", "?TeamsCID=first");
        let parts = UrlParts::new(&initial);
        let mut selection = parts.defaults();
        selection.nested[0].paths[3] = true; // 画像と同じ、内側のパスだけを固定。
        let route = DefaultRule {
            id: 1,
            profile_id: 2,
            pattern: StoredRule::new(parts.condition(&selection).unwrap(), false)
                .encode()
                .unwrap(),
            enabled: true,
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
        };
        let profiles = [profile(1, true, true), profile(2, false, true)];
        for query in ["?TeamsCID=second", "", "?TeamsCID=a&TeamsCID=b"] {
            let changed = wrapper("person_example_com", query);
            for interruption in [false, true] {
                assert_eq!(
                    resolve_profile(
                        &profiles,
                        std::slice::from_ref(&route),
                        &changed,
                        interruption
                    )
                    .unwrap()
                    .id,
                    2
                );
            }
        }
        let conflict = DefaultRule {
            profile_id: 1,
            ..route.clone()
        };
        assert!(resolve_profile(&profiles, &[route.clone(), conflict], &initial, false).is_none());
        assert!(
            resolve_profile(
                &profiles,
                std::slice::from_ref(&route),
                &wrapper("other_person", ""),
                false
            )
            .is_none()
        );
        let disabled = DefaultRule {
            enabled: false,
            ..route
        };
        assert!(resolve_profile(&profiles, &[disabled], &initial, false).is_none());
    }

    #[test]
    fn teams_routes_only_registered_account_and_never_falls_back_to_favorite() {
        use http_selector::association::{StoredRule, UrlParts};
        let wrapper = |cid: &str| {
            let mut url = url::Url::parse(http_selector::nested_url::TEAMS_WRAPPER_URL).unwrap();
            url.query_pairs_mut().append_pair(
                "url",
                &format!("https://tenant.sharepoint.com/a.pdf?TeamsCID={cid}"),
            );
            ValidatedUrl::parse(url.to_string()).unwrap()
        };
        let first = wrapper("11111111-2222-3333-4444-555555555555");
        let second = wrapper("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
        let profiles = [profile(1, true, true), profile(2, false, true)];
        let base = DefaultRule {
            id: 1,
            profile_id: 1,
            pattern: "https://teams.public.onecdn.static.microsoft/*".into(),
            enabled: true,
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
        };
        let parts = UrlParts::new(&first);
        let mut selection = parts.defaults();
        selection.nested[0].host = true;
        selection.nested[0].queries[0] = true;
        let account = DefaultRule {
            profile_id: 2,
            pattern: StoredRule::new(parts.condition(&selection).unwrap(), false)
                .encode()
                .unwrap(),
            ..base.clone()
        };
        assert_eq!(
            resolve_profile(&profiles, &[base.clone(), account.clone()], &first, false)
                .unwrap()
                .id,
            2
        );
        assert!(
            resolve_profile(&profiles, &[base.clone(), account.clone()], &second, false).is_none()
        );
        assert!(resolve_profile(&profiles, &[base], &first, false).is_none());
        let conflict = DefaultRule {
            profile_id: 1,
            ..account.clone()
        };
        assert!(resolve_profile(&profiles, &[account, conflict], &first, false).is_none());
        assert!(resolve_profile(&profiles, &[], &wrapper(""), false).is_none());
    }
}
