use crate::model::{BrowserType, DetectedProfile};
use anyhow::Context;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
    Ok(profiles)
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
