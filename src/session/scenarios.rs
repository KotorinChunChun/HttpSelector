//! 実画面・実通信・実ブラウザーに依存せず、製品Sessionと署名付きDBを通す試験。
use super::*;
use crate::association::{StoredRule, UrlParts};
use crate::browser::{BrowserCommand, build_command, build_test_command};
use crate::default_rule_ui::RulesOutcome;
use crate::model::{BrowserType, GUEST_PROFILE_ID};
use crate::quad9::Reputation;
use crate::risk::RiskReport;
use crate::risk_ui::Decision;
use crate::settings_ui::SettingsOutcome;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::PathBuf;

struct AssociationStep {
    rules: Vec<StoredRule>,
    action: association_ui::Action,
    pending: Option<ValidatedUrl>,
    rejected: bool,
}
impl AssociationStep {
    fn open() -> Self {
        Self {
            rules: vec![],
            action: association_ui::Action::Open,
            pending: None,
            rejected: false,
        }
    }
}

struct ScriptedHost {
    selections: RefCell<VecDeque<SelectorResult>>,
    inputs: RefCell<Vec<SelectorInput>>,
    associations: RefCell<VecDeque<AssociationStep>>,
    presets: RefCell<Vec<Option<DefaultRule>>>,
    settings: RefCell<VecDeque<SettingsAction>>,
    edit_settings: Cell<bool>,
    test_profile: Cell<bool>,
    settings_pending: RefCell<Option<ValidatedUrl>>,
    rules: RefCell<VecDeque<RulesOutcome>>,
    launches: RefCell<Vec<(i64, BrowserCommand)>>,
    test_launches: RefCell<Vec<BrowserCommand>>,
    launch_fails: Cell<bool>,
    inspections: RefCell<Vec<(bool, Vec<String>)>>,
    lookups: RefCell<Vec<String>>,
    blocked: Cell<bool>,
    decisions: RefCell<VecDeque<Decision>>,
    detected: RefCell<Vec<DetectedProfile>>,
    scan_fails: Cell<bool>,
    recovery: Cell<IntegrityRecoveryAction>,
    recovery_prompts: RefCell<Vec<bool>>,
    notifications: RefCell<Vec<(bool, String)>>,
}
impl Default for ScriptedHost {
    fn default() -> Self {
        Self {
            selections: Default::default(),
            inputs: Default::default(),
            associations: Default::default(),
            presets: Default::default(),
            settings: Default::default(),
            edit_settings: Cell::new(false),
            test_profile: Cell::new(false),
            settings_pending: Default::default(),
            rules: Default::default(),
            launches: Default::default(),
            test_launches: Default::default(),
            launch_fails: Cell::new(false),
            inspections: Default::default(),
            lookups: Default::default(),
            blocked: Cell::new(false),
            decisions: Default::default(),
            detected: RefCell::new(detected()),
            scan_fails: Cell::new(false),
            recovery: Cell::new(IntegrityRecoveryAction::Exit),
            recovery_prompts: Default::default(),
            notifications: Default::default(),
        }
    }
}
impl SessionHost for ScriptedHost {
    fn start_scan(&self, _: &AppPaths) -> Option<BackgroundProfileScan> {
        None
    }
    fn processing_url(&self, _: bool) {}
    fn selector(&self, input: SelectorInput) -> anyhow::Result<SelectorResult> {
        self.inputs.borrow_mut().push(input);
        self.selections
            .borrow_mut()
            .pop_front()
            .context("予期しない選択画面要求")
    }
    fn rules(
        &self,
        _: Vec<Profile>,
        _: Vec<DefaultRule>,
        _: Option<RuleDraft>,
    ) -> anyhow::Result<RulesOutcome> {
        self.rules
            .borrow_mut()
            .pop_front()
            .context("予期しないルール画面要求")
    }
    fn settings(
        &self,
        mut p: SettingsPreferences,
        save: &mut SavePreferences<'_>,
        test: &mut TestProfile<'_>,
    ) -> anyhow::Result<SettingsOutcome> {
        let action = self
            .settings
            .borrow_mut()
            .pop_front()
            .context("予期しない設定画面要求")?;
        if self.edit_settings.replace(false) {
            p.profiles[0].alias_name = "開発用".into();
            p.profiles[0].comment = "自動保存するメモ".into();
            p.profiles[0].favorite = true;
            p.record_url_history = true;
            p.escape_hold_duration_tenths = 35;
            p.open_profile_on_double_click = false;
            p.quad9_enabled = true;
            save(&p)?;
        }
        if self.test_profile.replace(false) {
            test(&p.profiles[0])?;
        }
        Ok(SettingsOutcome {
            action,
            profiles: p.profiles,
            pending_url: self.settings_pending.borrow_mut().take(),
            record_url_history: p.record_url_history,
            escape_hold_duration_tenths: p.escape_hold_duration_tenths,
            open_profile_on_double_click: p.open_profile_on_double_click,
            quad9_enabled: p.quad9_enabled,
        })
    }
    fn association(
        &self,
        _: &ValidatedUrl,
        _: &Profile,
        existing: Option<&DefaultRule>,
        save: &mut SaveAssociation<'_>,
    ) -> anyhow::Result<association_ui::Outcome> {
        self.presets.borrow_mut().push(existing.cloned());
        let step = self
            .associations
            .borrow_mut()
            .pop_front()
            .context("予期しない関連付け要求")?;
        for rule in step.rules {
            let result = save(&rule);
            if step.rejected {
                assert!(result.is_err());
            } else {
                result?;
            }
        }
        Ok(association_ui::Outcome {
            action: step.action,
            pending_url: step.pending,
        })
    }
    fn ensure_launchable(&self, profile: &Profile) -> anyhow::Result<()> {
        anyhow::ensure!(profile.is_selectable(), "利用できないプロファイル");
        Ok(())
    }
    fn inspect(&self, url: &ValidatedUrl, approved: &[String], quad9: bool) -> RiskReport {
        self.inspections
            .borrow_mut()
            .push((quad9, approved.to_vec()));
        risk::inspect_with_policy(url, approved, quad9, &mut |domain| {
            self.lookups.borrow_mut().push(domain.into());
            if self.blocked.get() {
                Reputation::Blocked
            } else {
                Reputation::Clear
            }
        })
    }
    fn confirm_risk(&self, _: &ValidatedUrl, _: &RiskReport) -> Decision {
        self.decisions
            .borrow_mut()
            .pop_front()
            .expect("警告には明示的な回答が必要")
    }
    fn launch(&self, profile: &Profile, url: &ValidatedUrl) -> anyhow::Result<()> {
        anyhow::ensure!(!self.launch_fails.replace(false), "起動失敗の注入");
        self.launches.borrow_mut().push((
            profile.id,
            build_command(PathBuf::from("synthetic-browser.exe"), profile, url),
        ));
        Ok(())
    }
    fn launch_test(&self, profile: &Profile) -> anyhow::Result<()> {
        self.test_launches.borrow_mut().push(build_test_command(
            PathBuf::from("synthetic-browser.exe"),
            profile,
        ));
        Ok(())
    }
    fn scan(&self, _: &AppPaths) -> anyhow::Result<Vec<DetectedProfile>> {
        anyhow::ensure!(!self.scan_fails.get(), "検出失敗の注入");
        Ok(self.detected.borrow().clone())
    }
    fn recovery(&self, has_backup: bool) -> IntegrityRecoveryAction {
        self.recovery_prompts.borrow_mut().push(has_backup);
        self.recovery.get()
    }
    fn notify(&self, error: bool, message: &str) {
        self.notifications
            .borrow_mut()
            .push((error, message.into()));
    }
}

fn detected() -> Vec<DetectedProfile> {
    [
        (BrowserType::Edge, "Default"),
        (BrowserType::Chrome, "Profile 1"),
        (BrowserType::Firefox, "dev"),
        (BrowserType::Edge, GUEST_PROFILE_ID),
    ]
    .into_iter()
    .map(|(browser_type, id)| DetectedProfile {
        browser_type,
        profile_id: id.into(),
        detected_name: id.into(),
        icon_path: None,
    })
    .collect()
}
struct Fixture {
    _temp: tempfile::TempDir,
    paths: AppPaths,
    storage: Storage,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path());
        paths.ensure_data_dir().unwrap();
        let storage = Storage::new(&paths.database);
        storage.initialize().unwrap();
        storage.sync_detected(&detected()).unwrap();
        storage.mark_initial_scan_completed().unwrap();
        IntegrityManager::new(&paths).sign_current().unwrap();
        Self {
            _temp: temp,
            paths,
            storage,
        }
    }
    fn update(&self, change: impl FnOnce() -> anyhow::Result<()>) {
        IntegrityManager::new(&self.paths).update(change).unwrap();
    }
    fn run(&self, host: &ScriptedHost, url: Option<ValidatedUrl>) {
        let integrity = IntegrityManager::new(&self.paths);
        Session::new(&self.paths, &self.storage, &integrity, host, url)
            .unwrap()
            .run()
            .unwrap();
        assert_eq!(integrity.verify_current().unwrap(), IntegrityStatus::Valid);
        assert!(host.selections.borrow().is_empty());
        assert!(host.associations.borrow().is_empty());
        assert!(host.settings.borrow().is_empty());
        assert!(host.rules.borrow().is_empty());
        assert!(host.decisions.borrow().is_empty());
    }
    fn favorite(&self, id: i64) {
        let mut p = self.storage.find_profile(id).unwrap().unwrap();
        p.favorite = true;
        self.update(|| self.storage.save_profile(&p));
    }
    fn rule(&self, profile: i64, url: &ValidatedUrl, suppress: bool) -> StoredRule {
        let rule = domain(url, suppress);
        self.update(|| self.storage.save_association(profile, &rule));
        rule
    }
    fn history_count(&self) -> i64 {
        rusqlite::Connection::open(&self.paths.database)
            .unwrap()
            .query_row("SELECT COUNT(*) FROM UrlHistory", [], |r| r.get(0))
            .unwrap()
    }
}
fn url(value: &str) -> ValidatedUrl {
    ValidatedUrl::parse(value).unwrap()
}
fn domain(url: &ValidatedUrl, suppress: bool) -> StoredRule {
    let p = UrlParts::new(url);
    StoredRule::new(p.condition(&p.defaults()).unwrap(), suppress)
}
fn select(host: &ScriptedHost, id: i64, value: &ValidatedUrl) {
    host.selections
        .borrow_mut()
        .push_back(SelectorResult::Profile {
            profile_id: id,
            url: Some(value.clone()),
        });
}
fn cancel(host: &ScriptedHost) {
    host.selections
        .borrow_mut()
        .push_back(SelectorResult::Cancelled);
}

#[test]
fn auto_rule_opens_original_url_without_questions_history_or_external_lookup() {
    let f = Fixture::new();
    let h = ScriptedHost::default();
    let u = url("HTTPS://example.com/a?code=one%20time#secret");
    f.rule(2, &u, false);
    f.run(&h, Some(u.clone()));
    assert!(h.inputs.borrow().is_empty());
    assert_eq!(h.launches.borrow()[0].0, 2);
    assert_eq!(
        h.launches.borrow()[0].1.arguments.last().unwrap(),
        u.original()
    );
    assert!(h.lookups.borrow().is_empty());
    assert_eq!(f.history_count(), 0);
}
#[test]
fn interruption_toggle_persists_and_routes_the_pending_url_to_first_favorite() {
    let f = Fixture::new();
    f.favorite(2);
    let h = ScriptedHost::default();
    let u = url("https://example.com/a");
    h.selections
        .borrow_mut()
        .push_back(SelectorResult::InterruptionChanged {
            enabled: false,
            url: Some(u.clone()),
        });
    f.run(&h, Some(u));
    assert!(!f.storage.interruption_enabled().unwrap());
    assert_eq!(h.launches.borrow()[0].0, 2);
}
#[test]
fn interruption_off_prefers_rule_and_shows_selector_if_no_available_favorite() {
    for has_rule in [false, true] {
        let f = Fixture::new();
        let h = ScriptedHost::default();
        let u = url("https://example.com/a");
        f.update(|| f.storage.set_interruption_enabled(false));
        if has_rule {
            f.favorite(1);
            f.rule(2, &u, false);
        } else {
            cancel(&h);
        }
        f.run(&h, Some(u));
        if has_rule {
            assert_eq!(h.launches.borrow()[0].0, 2);
        } else {
            assert!(h.launches.borrow().is_empty());
        }
    }
}
#[test]
fn saving_then_back_keeps_rule_but_never_launches_and_keeps_selection() {
    let f = Fixture::new();
    let h = ScriptedHost::default();
    let u = url("https://example.com/document?secret=not-saved");
    select(&h, 2, &u);
    cancel(&h);
    h.associations.borrow_mut().push_back(AssociationStep {
        rules: vec![domain(&u, false)],
        action: association_ui::Action::Back,
        ..AssociationStep::open()
    });
    f.run(&h, Some(u.clone()));
    assert!(h.launches.borrow().is_empty());
    let saved = f.storage.load_default_rules().unwrap();
    assert_eq!(saved.len(), 1);
    assert!(!saved[0].pattern.contains("not-saved"));
    assert_eq!(h.inputs.borrow()[1].selected_profile_id, Some(2));
    let next = ScriptedHost::default();
    f.run(&next, Some(url("https://example.com/another")));
    assert_eq!(next.launches.borrow()[0].0, 2);
}
#[test]
fn suppression_skips_association_but_still_requires_profile_selection() {
    let f = Fixture::new();
    let h = ScriptedHost::default();
    let u = url("https://example.com/a");
    f.rule(1, &u, true);
    select(&h, 3, &u);
    f.run(&h, Some(u));
    assert_eq!(h.inputs.borrow().len(), 1);
    assert!(h.presets.borrow().is_empty());
    assert_eq!(h.launches.borrow()[0].0, 3);
}
#[test]
fn shift_replaces_only_target_and_can_save_twice_without_stale_snapshot() {
    let f = Fixture::new();
    let h = ScriptedHost::default();
    let u = url("https://example.com/a?tenant=1").with_forced_confirmation(true);
    let rule = f.rule(1, &u, false);
    let before = f.storage.load_default_rules().unwrap().remove(0);
    select(&h, 2, &u);
    h.associations.borrow_mut().push_back(AssociationStep {
        rules: vec![rule.clone(), rule],
        ..AssociationStep::open()
    });
    f.run(&h, Some(u));
    let after = f.storage.load_default_rules().unwrap().remove(0);
    assert_eq!(
        (before.id, before.pattern, before.sort_order),
        (after.id, after.pattern, after.sort_order)
    );
    assert_eq!(after.profile_id, 2);
    assert_eq!(h.presets.borrow()[0].as_ref().unwrap().id, before.id);
    assert_eq!(h.launches.borrow()[0].0, 2);
}
#[test]
fn shift_refuses_ambiguous_replacement_without_changing_either_rule() {
    let f = Fixture::new();
    let h = ScriptedHost::default();
    let u = url("https://example.com/a").with_forced_confirmation(true);
    f.rule(1, &u, false);
    let mut rules = f.storage.load_default_rules().unwrap();
    let mut other = rules[0].clone();
    other.id = 0;
    other.profile_id = 2;
    rules.push(other);
    f.update(|| f.storage.replace_default_rules(&rules));
    let before = std::fs::read(&f.paths.database).unwrap();
    select(&h, 3, &u);
    cancel(&h);
    h.associations.borrow_mut().push_back(AssociationStep {
        rules: vec![domain(&u, false)],
        action: association_ui::Action::Back,
        rejected: true,
        ..AssociationStep::open()
    });
    f.run(&h, Some(u));
    assert_eq!(before, std::fs::read(&f.paths.database).unwrap());
    assert!(h.launches.borrow().is_empty());
}
#[test]
fn warning_cancel_has_no_rule_history_approval_or_launch_side_effect() {
    let f = Fixture::new();
    let h = ScriptedHost::default();
    let u = url("https://microsfot.com/a?code=secret");
    select(&h, 1, &u);
    let before = std::fs::read(&f.paths.database).unwrap();
    h.decisions.borrow_mut().push_back(Decision {
        accepted: false,
        approve_domains: false,
    });
    f.run(&h, Some(u));
    assert_eq!(before, std::fs::read(&f.paths.database).unwrap());
    assert!(h.launches.borrow().is_empty());
    assert!(h.presets.borrow().is_empty());
}
#[test]
fn explicit_domain_approval_is_signed_and_suppresses_only_later_name_warning() {
    let f = Fixture::new();
    let h = ScriptedHost::default();
    let u = url("https://microsfot.com/a");
    select(&h, 1, &u);
    h.decisions.borrow_mut().push_back(Decision {
        accepted: true,
        approve_domains: true,
    });
    h.associations
        .borrow_mut()
        .push_back(AssociationStep::open());
    f.run(&h, Some(u.clone()));
    assert_eq!(f.storage.approved_domains().unwrap(), ["microsfot.com"]);
    let next = ScriptedHost::default();
    select(&next, 1, &u);
    next.associations
        .borrow_mut()
        .push_back(AssociationStep::open());
    f.run(&next, Some(u));
    assert_eq!(next.launches.borrow().len(), 1);
    assert_eq!(next.inspections.borrow()[0].1, ["microsfot.com"]);
}
#[test]
fn quad9_opt_in_sends_domain_only_and_block_still_warns_for_approved_name() {
    let f = Fixture::new();
    let h = ScriptedHost::default();
    let u = url("https://example.com/a?code=not-sent#private");
    f.update(|| {
        f.storage.set_setting("quad9_enabled", "1")?;
        f.storage.set_approved_domains(&["example.com".into()])
    });
    f.rule(1, &u, false);
    h.blocked.set(true);
    h.decisions.borrow_mut().push_back(Decision {
        accepted: false,
        approve_domains: false,
    });
    f.run(&h, Some(u));
    assert_eq!(*h.lookups.borrow(), ["example.com"]);
    assert!(h.launches.borrow().is_empty());
}
#[test]
fn launch_failure_returns_to_selector_without_recording_history() {
    let f = Fixture::new();
    let h = ScriptedHost::default();
    let u = url("https://example.com/a");
    f.update(|| f.storage.set_record_url_history_enabled(true));
    select(&h, 1, &u);
    cancel(&h);
    h.associations
        .borrow_mut()
        .push_back(AssociationStep::open());
    h.launch_fails.set(true);
    f.run(&h, Some(u.clone()));
    assert_eq!(f.history_count(), 0);
    assert_eq!(h.inputs.borrow()[1].url, Some(u));
    assert_eq!(h.inputs.borrow()[1].selected_profile_id, Some(1));
    assert!(h.notifications.borrow()[0].0);
}
#[test]
fn later_url_after_association_is_routed_after_current_url_without_overwriting_it() {
    let f = Fixture::new();
    let h = ScriptedHost::default();
    let first = url("https://first.example.com/a");
    let next = url("https://next.example.com/b");
    f.rule(2, &next, false);
    select(&h, 1, &first);
    h.associations.borrow_mut().push_back(AssociationStep {
        pending: Some(next.clone()),
        ..AssociationStep::open()
    });
    f.run(&h, Some(first.clone()));
    let launches = h.launches.borrow();
    assert_eq!(launches.len(), 2);
    assert_eq!(launches[0].1.arguments.last().unwrap(), first.original());
    assert_eq!(launches[1].1.arguments.last().unwrap(), next.original());
}
#[test]
fn enabled_history_records_redacted_pattern_only_after_success() {
    let f = Fixture::new();
    let h = ScriptedHost::default();
    let u = url("https://example.com/folder/document?code=do-not-save#private");
    f.update(|| f.storage.set_record_url_history_enabled(true));
    f.rule(1, &u, false);
    f.run(&h, Some(u));
    assert_eq!(f.history_count(), 1);
    let conn = rusqlite::Connection::open(&f.paths.database).unwrap();
    let pattern: String = conn
        .query_row("SELECT Url FROM UrlHistory", [], |r| r.get(0))
        .unwrap();
    assert!(
        !pattern.contains("do-not-save")
            && !pattern.contains("document")
            && !pattern.contains("private")
    );
}
#[test]
fn settings_save_and_test_callbacks_persist_preferences_and_reload_selector() {
    let f = Fixture::new();
    let h = ScriptedHost::default();
    h.edit_settings.set(true);
    h.test_profile.set(true);
    h.selections
        .borrow_mut()
        .push_back(SelectorResult::Settings { url: None });
    cancel(&h);
    h.settings.borrow_mut().push_back(SettingsAction::Closed);
    f.run(&h, None);
    let p = f.storage.find_profile(1).unwrap().unwrap();
    assert_eq!(p.alias_name, "開発用");
    assert_eq!(p.comment, "自動保存するメモ");
    assert!(p.favorite);
    assert!(f.storage.record_url_history_enabled().unwrap() && f.storage.quad9_enabled().unwrap());
    assert_eq!(h.inputs.borrow()[1].escape_hold_duration_tenths, 35);
    assert!(!h.inputs.borrow()[1].open_profile_on_double_click);
    assert_eq!(
        h.test_launches.borrow()[0].arguments.last().unwrap(),
        "about:blank"
    );
    assert!(h.launches.borrow().is_empty());
}
#[test]
fn clearing_approvals_does_not_change_other_settings_or_launch_a_browser() {
    let f = Fixture::new();
    let h = ScriptedHost::default();
    f.update(|| f.storage.set_approved_domains(&["example.com".into()]));
    h.selections
        .borrow_mut()
        .push_back(SelectorResult::Settings { url: None });
    cancel(&h);
    h.settings
        .borrow_mut()
        .push_back(SettingsAction::ClearDomainApprovals);
    f.run(&h, None);
    assert!(f.storage.approved_domains().unwrap().is_empty());
    assert!(!f.storage.quad9_enabled().unwrap());
    assert!(h.launches.borrow().is_empty());
}
#[test]
fn pending_settings_url_is_evaluated_against_saved_rules() {
    let f = Fixture::new();
    let h = ScriptedHost::default();
    let pending = url("https://example.com/a");
    f.rule(3, &pending, false);
    h.selections
        .borrow_mut()
        .push_back(SelectorResult::Settings { url: None });
    h.settings.borrow_mut().push_back(SettingsAction::Closed);
    *h.settings_pending.borrow_mut() = Some(pending);
    f.run(&h, None);
    assert_eq!(h.launches.borrow()[0].0, 3);
}
#[test]
fn rule_list_and_new_rule_draft_save_before_routing_a_later_url() {
    for draft in [false, true] {
        let f = Fixture::new();
        let h = ScriptedHost::default();
        let u = url("https://example.com/a");
        f.rule(1, &u, false);
        let mut rules = f.storage.load_default_rules().unwrap();
        rules[0].profile_id = 2;
        h.selections.borrow_mut().push_back(if draft {
            SelectorResult::ConfigureDefaultRule {
                profile_id: 2,
                url: u.clone(),
            }
        } else {
            SelectorResult::RuleList {
                url: Some(u.clone()),
            }
        });
        h.rules.borrow_mut().push_back(RulesOutcome {
            action: RulesAction::Saved,
            rules,
            pending_url: Some(u.clone()),
        });
        f.run(&h, None);
        assert_eq!(h.launches.borrow()[0].0, 2);
    }
}
#[test]
fn favorite_changes_preserve_url_and_are_visible_on_the_next_selector() {
    let f = Fixture::new();
    let h = ScriptedHost::default();
    let u = url("https://example.com/a");
    h.selections
        .borrow_mut()
        .push_back(SelectorResult::FavoriteChanged {
            profile_id: 2,
            favorite: true,
            url: Some(u.clone()),
        });
    cancel(&h);
    f.run(&h, Some(u.clone()));
    assert!(f.storage.find_profile(2).unwrap().unwrap().favorite);
    assert_eq!(h.inputs.borrow()[1].selected_profile_id, Some(2));
    assert_eq!(h.inputs.borrow()[1].url, Some(u));
}
#[test]
fn unchanged_refresh_is_silent_and_scan_failure_keeps_the_url() {
    for fail in [false, true] {
        let f = Fixture::new();
        let h = ScriptedHost::default();
        let u = url("https://example.com/a");
        h.scan_fails.set(fail);
        h.selections
            .borrow_mut()
            .push_back(SelectorResult::RefreshRequested {
                url: Some(u.clone()),
                selected_profile_id: Some(2),
            });
        cancel(&h);
        f.run(&h, Some(u.clone()));
        assert_eq!(h.inputs.borrow()[1].url, Some(u));
        assert_eq!(h.notifications.borrow().len(), usize::from(fail));
    }
}
#[test]
fn background_detection_is_saved_and_preserves_alias_and_selection() {
    let f = Fixture::new();
    let h = ScriptedHost::default();
    let mut p = f.storage.find_profile(1).unwrap().unwrap();
    p.alias_name = "仕事用".into();
    f.update(|| f.storage.save_profile(&p));
    let mut changed = detected();
    changed[0].detected_name = "検出名変更".into();
    h.selections
        .borrow_mut()
        .push_back(SelectorResult::ProfilesDetected {
            detected: changed,
            url: None,
            selected_profile_id: Some(1),
        });
    cancel(&h);
    f.run(&h, None);
    let p = f.storage.find_profile(1).unwrap().unwrap();
    assert_eq!(p.alias_name, "仕事用");
    assert_eq!(p.detected_name, "検出名変更");
    assert_eq!(h.inputs.borrow()[1].selected_profile_id, Some(1));
}
#[test]
fn guest_selection_uses_guest_command_without_a_signed_in_profile_argument() {
    let f = Fixture::new();
    let h = ScriptedHost::default();
    let u = url("https://example.com/a");
    select(&h, 4, &u);
    h.associations
        .borrow_mut()
        .push_back(AssociationStep::open());
    f.run(&h, Some(u));
    let command = &h.launches.borrow()[0].1;
    assert!(command.arguments.iter().any(|a| a == "--guest"));
    assert!(
        !command
            .arguments
            .iter()
            .any(|a| a.starts_with("--profile-directory"))
    );
}
#[test]
fn first_start_initializes_scans_and_signs_without_user_interaction() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_dir(temp.path());
    let h = ScriptedHost::default();
    let integrity = IntegrityManager::new(&paths);
    let storage = open_storage(&paths, &integrity, &h).unwrap().unwrap();
    assert_eq!(storage.profile_count().unwrap(), 4);
    assert!(storage.initial_scan_completed().unwrap());
    assert_eq!(integrity.verify_current().unwrap(), IntegrityStatus::Valid);
    assert!(h.recovery_prompts.borrow().is_empty());
}
#[test]
fn corrupt_database_supports_restore_initialize_and_exit_using_the_same_startup_flow() {
    for action in [
        IntegrityRecoveryAction::Restore,
        IntegrityRecoveryAction::Initialize,
        IntegrityRecoveryAction::Exit,
    ] {
        let f = Fixture::new();
        let h = ScriptedHost::default();
        h.recovery.set(action);
        let integrity = IntegrityManager::new(&f.paths);
        integrity.backup_current().unwrap();
        std::fs::write(&f.paths.database, b"corrupt").unwrap();
        let opened = open_storage(&f.paths, &integrity, &h).unwrap();
        assert_eq!(*h.recovery_prompts.borrow(), [true]);
        if action == IntegrityRecoveryAction::Exit {
            assert!(opened.is_none());
            assert_eq!(std::fs::read(&f.paths.database).unwrap(), b"corrupt");
        } else {
            assert_eq!(opened.unwrap().profile_count().unwrap(), 4);
            assert_eq!(integrity.verify_current().unwrap(), IntegrityStatus::Valid);
        }
    }
}
#[test]
fn failed_write_and_failed_signature_restore_the_real_sqlite_database() {
    let f = Fixture::new();
    let integrity = IntegrityManager::new(&f.paths);
    let original = std::fs::read(&f.paths.database).unwrap();
    let changed = integrity.update(|| -> anyhow::Result<()> {
        f.storage.set_interruption_enabled(false)?;
        anyhow::bail!("変更後の失敗")
    });
    assert!(changed.is_err());
    assert!(f.storage.interruption_enabled().unwrap());
    assert_eq!(std::fs::read(&f.paths.database).unwrap(), original);
    let signed = integrity.update(|| {
        std::fs::remove_file(&f.paths.database)?;
        Ok(())
    });
    assert!(signed.is_err());
    assert_eq!(std::fs::read(&f.paths.database).unwrap(), original);
    assert_eq!(integrity.verify_current().unwrap(), IntegrityStatus::Valid);
}
