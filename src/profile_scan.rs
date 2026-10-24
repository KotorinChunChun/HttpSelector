use crate::app_paths::AppPaths;
use crate::model::{BrowserType, DetectedProfile, Profile};
use anyhow::Context;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProfileChanges {
    pub added: usize,
    pub updated: usize,
    pub unavailable: usize,
}

impl ProfileChanges {
    pub const fn changed(self) -> bool {
        self.added != 0 || self.updated != 0 || self.unavailable != 0
    }

    pub fn message(self) -> String {
        if self.changed() {
            format!(
                "プロファイルを更新しました（追加 {}件、変更・再検出 {}件、未検出 {}件）。",
                self.added, self.updated, self.unavailable
            )
        } else {
            "プロファイルに変更はありません。".to_owned()
        }
    }
}

type BackgroundResult = Result<Vec<DetectedProfile>, String>;

#[derive(Clone)]
pub struct BackgroundProfileScan {
    result: Arc<Mutex<Option<BackgroundResult>>>,
    finished: Arc<AtomicBool>,
}

impl BackgroundProfileScan {
    pub fn try_take(&self) -> Option<BackgroundResult> {
        self.result.lock().ok()?.take()
    }

    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }

    pub fn is_consumed(&self) -> bool {
        self.is_finished() && self.result.lock().is_ok_and(|result| result.is_none())
    }

    #[cfg(test)]
    pub(crate) fn completed(detected: Vec<DetectedProfile>) -> Self {
        Self {
            result: Arc::new(Mutex::new(Some(Ok(detected)))),
            finished: Arc::new(AtomicBool::new(true)),
        }
    }
}

pub fn start_background(paths: AppPaths) -> BackgroundProfileScan {
    let result = Arc::new(Mutex::new(None));
    let finished = Arc::new(AtomicBool::new(false));
    let worker_result = Arc::clone(&result);
    let worker_finished = Arc::clone(&finished);
    std::thread::spawn(move || {
        let scan_result = scan_all()
            .and_then(|mut detected| {
                crate::browser::cache_detected_icons(&paths, &mut detected)?;
                Ok(detected)
            })
            .map_err(|error| format!("{error:#}"));
        if let Ok(mut slot) = worker_result.lock() {
            *slot = Some(scan_result);
        }
        worker_finished.store(true, Ordering::Release);
    });
    BackgroundProfileScan { result, finished }
}

pub fn changes(current: &[Profile], detected: &[DetectedProfile]) -> ProfileChanges {
    let detected_keys: HashSet<_> = detected
        .iter()
        .map(|profile| (profile.browser_type, profile.profile_id.as_str()))
        .collect();
    let mut result = ProfileChanges::default();
    let mut compared_keys = HashSet::new();
    for profile in detected {
        if !compared_keys.insert((profile.browser_type, profile.profile_id.as_str())) {
            continue;
        }
        match current.iter().find(|existing| {
            existing.browser_type == profile.browser_type
                && existing.profile_id == profile.profile_id
        }) {
            None => result.added += 1,
            Some(existing)
                if !existing.available
                    || existing.detected_name != profile.detected_name
                    || profile
                        .icon_path
                        .as_ref()
                        .is_some_and(|icon| existing.icon_path.as_ref() != Some(icon)) =>
            {
                result.updated += 1;
            }
            Some(_) => {}
        }
    }
    result.unavailable = current
        .iter()
        .filter(|profile| {
            profile.available
                && !detected_keys.contains(&(profile.browser_type, profile.profile_id.as_str()))
        })
        .count();
    result
}

pub fn scan_all() -> anyhow::Result<Vec<DetectedProfile>> {
    let mut profiles = Vec::new();
    if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        scan_chromium(
            BrowserType::Edge,
            &local.join("Microsoft").join("Edge").join("User Data"),
            &mut profiles,
        )?;
        scan_chromium(
            BrowserType::Chrome,
            &local.join("Google").join("Chrome").join("User Data"),
            &mut profiles,
        )?;
    }
    if let Some(roaming) = std::env::var_os("APPDATA").map(PathBuf::from) {
        scan_firefox(&roaming.join("Mozilla").join("Firefox"), &mut profiles)?;
    }
    for browser_type in [BrowserType::Edge, BrowserType::Chrome] {
        append_guest_profile(
            &mut profiles,
            browser_type,
            crate::browser::resolve_executable(browser_type).is_ok(),
            crate::browser::guest_mode_allowed(browser_type),
        );
    }
    Ok(profiles)
}

fn append_guest_profile(
    profiles: &mut Vec<DetectedProfile>,
    browser_type: BrowserType,
    installed: bool,
    allowed: bool,
) {
    if installed
        && allowed
        && matches!(browser_type, BrowserType::Edge | BrowserType::Chrome)
        && !profiles.iter().any(|p| {
            p.browser_type == browser_type && p.profile_id == crate::model::GUEST_PROFILE_ID
        })
    {
        profiles.push(DetectedProfile {
            browser_type,
            profile_id: crate::model::GUEST_PROFILE_ID.to_owned(),
            detected_name: "ゲスト".to_owned(),
            icon_path: None,
        });
    }
}

pub fn scan_chromium(
    browser_type: BrowserType,
    user_data: &Path,
    output: &mut Vec<DetectedProfile>,
) -> anyhow::Result<()> {
    if !user_data.is_dir() {
        return Ok(());
    }

    let local_state_path = user_data.join("Local State");
    if local_state_path.is_file() {
        let json = std::fs::read_to_string(&local_state_path).with_context(|| {
            format!(
                "ブラウザ設定を読み取れません: {}",
                local_state_path.display()
            )
        })?;
        let value: Value = serde_json::from_str(&json).with_context(|| {
            format!(
                "ブラウザ設定を解析できません: {}",
                local_state_path.display()
            )
        })?;
        if let Some(info_cache) = value
            .get("profile")
            .and_then(|profile| profile.get("info_cache"))
            .and_then(Value::as_object)
        {
            for (profile_id, info) in info_cache {
                if user_data.join(profile_id).is_dir() {
                    let detected_name = info
                        .get("name")
                        .and_then(Value::as_str)
                        .filter(|name| !name.trim().is_empty())
                        .unwrap_or(profile_id)
                        .to_owned();
                    output.push(DetectedProfile {
                        browser_type,
                        profile_id: profile_id.clone(),
                        detected_name,
                        icon_path: None,
                    });
                }
            }
        }
    }

    if !output
        .iter()
        .any(|profile| profile.browser_type == browser_type)
        && user_data.join("Default").is_dir()
    {
        output.push(DetectedProfile {
            browser_type,
            profile_id: "Default".to_owned(),
            detected_name: "Default".to_owned(),
            icon_path: None,
        });
    }
    Ok(())
}

pub fn scan_firefox(firefox_root: &Path, output: &mut Vec<DetectedProfile>) -> anyhow::Result<()> {
    let profiles_ini = firefox_root.join("profiles.ini");
    if !profiles_ini.is_file() {
        return Ok(());
    }
    let content = std::fs::read_to_string(&profiles_ini)
        .with_context(|| format!("Firefox設定を読み取れません: {}", profiles_ini.display()))?;

    let mut section = String::new();
    let mut values: HashMap<String, String> = HashMap::new();
    let flush =
        |section: &str, values: &HashMap<String, String>, output: &mut Vec<DetectedProfile>| {
            if !section.starts_with("Profile") {
                return;
            }
            let Some(name) = values.get("Name").filter(|value| !value.trim().is_empty()) else {
                return;
            };
            let Some(path_value) = values.get("Path") else {
                return;
            };
            let is_relative = values.get("IsRelative").map(String::as_str) != Some("0");
            let path = if is_relative {
                firefox_root.join(path_value.replace('/', "\\"))
            } else {
                PathBuf::from(path_value)
            };
            if path.is_dir() {
                output.push(DetectedProfile {
                    browser_type: BrowserType::Firefox,
                    profile_id: name.clone(),
                    detected_name: name.clone(),
                    icon_path: None,
                });
            }
        };

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            flush(&section, &values, output);
            section = line[1..line.len() - 1].to_owned();
            values.clear();
        } else if let Some((key, value)) = line.split_once('=') {
            values.insert(key.trim().to_owned(), value.trim().to_owned());
        }
    }
    flush(&section, &values, output);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_is_stable_unique_and_only_added_for_available_allowed_browsers() {
        let mut profiles = vec![];
        for _ in 0..2 {
            append_guest_profile(&mut profiles, BrowserType::Chrome, true, true);
        }
        append_guest_profile(&mut profiles, BrowserType::Edge, false, true);
        append_guest_profile(&mut profiles, BrowserType::Edge, true, false);
        append_guest_profile(&mut profiles, BrowserType::Firefox, true, true);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].profile_id, crate::model::GUEST_PROFILE_ID);
        assert_eq!(profiles[0].detected_name, "ゲスト");
        append_guest_profile(&mut profiles, BrowserType::Edge, true, true);
        let temp = tempfile::tempdir().unwrap();
        let storage = crate::storage::Storage::new(temp.path().join("guest.db"));
        storage.initialize().unwrap();
        storage.sync_detected(&profiles).unwrap();
        let before = storage.load_all_profiles().unwrap();
        storage.sync_detected(&profiles).unwrap();
        let after = storage.load_all_profiles().unwrap();
        assert_eq!(
            before.iter().map(|p| p.id).collect::<Vec<_>>(),
            after.iter().map(|p| p.id).collect::<Vec<_>>()
        );
        assert!(after.iter().all(Profile::is_guest));
    }

    fn stored(browser_type: BrowserType, id: &str, name: &str, available: bool) -> Profile {
        Profile {
            id: 1,
            browser_type,
            profile_id: id.to_owned(),
            detected_name: name.to_owned(),
            alias_name: "利用者設定".to_owned(),
            comment: String::new(),
            sort_order: 0,
            favorite: true,
            enabled: true,
            available,
            icon_path: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn detects_added_updated_restored_and_missing_profiles() {
        let current = vec![
            stored(BrowserType::Edge, "Default", "個人", true),
            stored(BrowserType::Chrome, "Profile 1", "会社", true),
            stored(BrowserType::Firefox, "old", "旧", false),
        ];
        let detected = vec![
            DetectedProfile {
                browser_type: BrowserType::Edge,
                profile_id: "Default".into(),
                detected_name: "名称変更".into(),
                icon_path: None,
            },
            DetectedProfile {
                browser_type: BrowserType::Firefox,
                profile_id: "old".into(),
                detected_name: "旧".into(),
                icon_path: None,
            },
            DetectedProfile {
                browser_type: BrowserType::Chrome,
                profile_id: "Profile 2".into(),
                detected_name: "追加".into(),
                icon_path: None,
            },
        ];
        assert_eq!(
            changes(&current, &detected),
            ProfileChanges {
                added: 1,
                updated: 2,
                unavailable: 1,
            }
        );
        assert!(changes(&current, &detected).changed());
    }

    #[test]
    fn ignores_identical_profiles_and_already_missing_entries() {
        let current = vec![
            stored(BrowserType::Edge, "Default", "個人", true),
            stored(BrowserType::Firefox, "old", "旧", false),
        ];
        let detected_profile = DetectedProfile {
            browser_type: BrowserType::Edge,
            profile_id: "Default".into(),
            detected_name: "個人".into(),
            icon_path: None,
        };
        let detected = vec![detected_profile.clone(), detected_profile];
        assert_eq!(changes(&current, &detected), ProfileChanges::default());
        assert!(!changes(&current, &detected).changed());
    }

    #[test]
    fn reads_chromium_profile_cache() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("Default")).unwrap();
        std::fs::create_dir(temp.path().join("Profile 2")).unwrap();
        std::fs::write(
            temp.path().join("Local State"),
            r#"{"profile":{"info_cache":{"Default":{"name":"個人"},"Profile 2":{"name":"会社"}}}}"#,
        )
        .unwrap();
        let mut profiles = Vec::new();
        scan_chromium(BrowserType::Chrome, temp.path(), &mut profiles).unwrap();
        assert_eq!(profiles.len(), 2);
        assert!(
            profiles
                .iter()
                .any(|profile| profile.detected_name == "会社")
        );
    }

    #[test]
    fn reads_relative_firefox_profiles() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("Profiles").join("abc.work")).unwrap();
        std::fs::write(
            temp.path().join("profiles.ini"),
            "[Profile0]\nName=work\nIsRelative=1\nPath=Profiles/abc.work\nDefault=1\n",
        )
        .unwrap();
        let mut profiles = Vec::new();
        scan_firefox(temp.path(), &mut profiles).unwrap();
        assert_eq!(profiles[0].profile_id, "work");
    }
}
