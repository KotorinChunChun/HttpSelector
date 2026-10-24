//! Sessionが利用する外部境界。製品とCLI試験で業務処理を共用する。
use crate::app_paths::AppPaths;
use crate::association::StoredRule;
use crate::association_ui;
use crate::default_rule_ui::{RuleDraft, RulesOutcome};
use crate::management::Management;
use crate::model::{DefaultRule, DetectedProfile, Profile};
use crate::profile_scan::BackgroundProfileScan;
use crate::risk::RiskReport;
use crate::risk_ui::Decision;
use crate::settings_ui::{SettingsOutcome, SettingsPreferences};
use crate::ui::{IntegrityRecoveryAction, SelectorResult};
use crate::url_input::ValidatedUrl;
use crate::{app_title, browser, profile_scan, risk, risk_ui, ui};

pub struct SelectorInput {
    pub url: Option<ValidatedUrl>,
    pub profiles: Vec<Profile>,
    pub escape_hold_duration_tenths: u32,
    pub open_profile_on_double_click: bool,
    pub selected_profile_id: Option<i64>,
    pub background_scan: Option<BackgroundProfileScan>,
    pub interruption_enabled: bool,
}

pub type SavePreferences<'a> = dyn FnMut(&SettingsPreferences) -> anyhow::Result<()> + 'a;
pub type TestProfile<'a> = dyn FnMut(&Profile) -> anyhow::Result<()> + 'a;
pub type SaveAssociation<'a> = dyn FnMut(&StoredRule) -> anyhow::Result<()> + 'a;

/// 既定実装を持たせず、試験で実画面・実通信・実ブラウザーへ漏れることを防ぐ。
pub trait SessionHost {
    fn start_scan(&self, paths: &AppPaths) -> Option<BackgroundProfileScan>;
    fn processing_url(&self, processing: bool);
    fn selector(&self, input: SelectorInput) -> anyhow::Result<SelectorResult>;
    fn rules(
        &self,
        profiles: Vec<Profile>,
        rules: Vec<DefaultRule>,
        draft: Option<RuleDraft>,
    ) -> anyhow::Result<RulesOutcome>;
    fn settings(
        &self,
        preferences: SettingsPreferences,
        save: &mut SavePreferences<'_>,
        test: &mut TestProfile<'_>,
    ) -> anyhow::Result<SettingsOutcome>;
    fn association(
        &self,
        url: &ValidatedUrl,
        profile: &Profile,
        existing: Option<&DefaultRule>,
        save: &mut SaveAssociation<'_>,
    ) -> anyhow::Result<association_ui::Outcome>;
    fn ensure_launchable(&self, profile: &Profile) -> anyhow::Result<()>;
    fn inspect(&self, url: &ValidatedUrl, approved: &[String], quad9: bool) -> RiskReport;
    fn confirm_risk(&self, url: &ValidatedUrl, report: &RiskReport) -> Decision;
    fn launch(&self, profile: &Profile, url: &ValidatedUrl) -> anyhow::Result<()>;
    fn launch_test(&self, profile: &Profile) -> anyhow::Result<()>;
    fn scan(&self, paths: &AppPaths) -> anyhow::Result<Vec<DetectedProfile>>;
    fn recovery(&self, has_backup: bool) -> IntegrityRecoveryAction;
    fn notify(&self, error: bool, message: &str);
}

pub struct NativeHost<'a> {
    pub management: &'a Management,
}

impl SessionHost for NativeHost<'_> {
    fn start_scan(&self, paths: &AppPaths) -> Option<BackgroundProfileScan> {
        (!crate::app_options::test_scan_disabled())
            .then(|| profile_scan::start_background(paths.clone()))
    }
    fn processing_url(&self, processing: bool) {
        self.management.set_processing_url(processing);
    }
    fn selector(&self, input: SelectorInput) -> anyhow::Result<SelectorResult> {
        ui::run_selector_with_selection_and_scan(
            input.url,
            input.profiles,
            input.escape_hold_duration_tenths,
            input.open_profile_on_double_click,
            input.selected_profile_id,
            input.background_scan,
            input.interruption_enabled,
        )
    }
    fn rules(
        &self,
        profiles: Vec<Profile>,
        rules: Vec<DefaultRule>,
        draft: Option<RuleDraft>,
    ) -> anyhow::Result<RulesOutcome> {
        crate::default_rule_ui::run_rules(profiles, rules, draft)
    }
    fn settings(
        &self,
        p: SettingsPreferences,
        save: &mut SavePreferences<'_>,
        test: &mut TestProfile<'_>,
    ) -> anyhow::Result<SettingsOutcome> {
        crate::settings_ui::run_settings_live(
            p.profiles,
            p.record_url_history,
            p.escape_hold_duration_tenths,
            p.open_profile_on_double_click,
            p.quad9_enabled,
            save,
            test,
        )
    }
    fn association(
        &self,
        url: &ValidatedUrl,
        profile: &Profile,
        existing: Option<&DefaultRule>,
        save: &mut SaveAssociation<'_>,
    ) -> anyhow::Result<association_ui::Outcome> {
        association_ui::run_with_existing(url, profile, existing, save)
    }
    fn ensure_launchable(&self, profile: &Profile) -> anyhow::Result<()> {
        browser::ensure_profile_is_launchable(profile)
    }
    fn inspect(&self, url: &ValidatedUrl, approved: &[String], quad9: bool) -> RiskReport {
        risk::inspect_approved_with_quad9(url, approved, quad9)
    }
    fn confirm_risk(&self, url: &ValidatedUrl, report: &RiskReport) -> Decision {
        risk_ui::confirm_with_approval(url, report)
    }
    fn launch(&self, profile: &Profile, url: &ValidatedUrl) -> anyhow::Result<()> {
        browser::launch(profile, url)
    }
    fn launch_test(&self, profile: &Profile) -> anyhow::Result<()> {
        browser::launch_test(profile)
    }
    fn scan(&self, paths: &AppPaths) -> anyhow::Result<Vec<DetectedProfile>> {
        use anyhow::Context;
        let mut detected = if crate::app_options::test_scan_disabled() {
            Vec::new()
        } else {
            profile_scan::scan_all().context("プロファイル検出に失敗しました")?
        };
        browser::cache_detected_icons(paths, &mut detected)?;
        Ok(detected)
    }
    fn recovery(&self, has_backup: bool) -> IntegrityRecoveryAction {
        ui::ask_integrity_recovery(has_backup)
    }
    fn notify(&self, error: bool, message: &str) {
        if error {
            ui::show_error(&app_title::main(), message);
        } else {
            ui::show_information(&app_title::main(), message);
        }
    }
}
