use crate::model::{BrowserType, DefaultRule, DetectedProfile, Profile};
use anyhow::Context;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Storage {
    database_path: PathBuf,
}

const SETTING_INITIAL_SCAN_COMPLETED: &str = "initial_scan_completed";
const SETTING_RECORD_URL_HISTORY: &str = "record_url_history";
const SETTING_ESCAPE_HOLD_DURATION_TENTHS: &str = "escape_hold_duration_tenths";
const SETTING_OPEN_PROFILE_ON_DOUBLE_CLICK: &str = "open_profile_on_double_click";
const SETTING_INTERRUPTION_ENABLED: &str = "interruption_enabled";
pub const DEFAULT_ESCAPE_HOLD_DURATION_TENTHS: u32 = 10;
pub const MIN_ESCAPE_HOLD_DURATION_TENTHS: u32 = 1;
pub const MAX_ESCAPE_HOLD_DURATION_TENTHS: u32 = 100;
pub const DEFAULT_OPEN_PROFILE_ON_DOUBLE_CLICK: bool = true;

impl Storage {
    pub fn new(database_path: impl Into<PathBuf>) -> Self {
        Self {
            database_path: database_path.into(),
        }
    }

    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    pub fn initialize(&self) -> anyhow::Result<()> {
        let mut connection = self.open()?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        anyhow::ensure!(version <= 4, "新しい版の設定DBはこの版では開けません");
        connection.pragma_update(None, "journal_mode", "DELETE")?;
        let transaction = connection.transaction()?;
        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS Profiles (
                 Id INTEGER PRIMARY KEY AUTOINCREMENT,
                 BrowserType INTEGER NOT NULL,
                 ProfileId TEXT NOT NULL,
                 DetectedName TEXT NOT NULL,
                 AliasName TEXT NOT NULL DEFAULT '',
                 Comment TEXT NOT NULL DEFAULT '',
                 SortOrder INTEGER NOT NULL DEFAULT 0,
                 Favorite INTEGER NOT NULL DEFAULT 0,
                 Enabled INTEGER NOT NULL DEFAULT 1,
                 Available INTEGER NOT NULL DEFAULT 1,
                 IconPath TEXT,
                 CreatedAt INTEGER NOT NULL,
                 UpdatedAt INTEGER NOT NULL,
                 UNIQUE(BrowserType, ProfileId)
             );
             CREATE TABLE IF NOT EXISTS Settings (
                 Key TEXT PRIMARY KEY,
                 Value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS UrlHistory (
                 Id INTEGER PRIMARY KEY AUTOINCREMENT,
                 ProfileId INTEGER NOT NULL REFERENCES Profiles(Id) ON DELETE CASCADE,
                 Url TEXT NOT NULL,
                  OpenedAt INTEGER NOT NULL
              );
              CREATE TABLE IF NOT EXISTS DefaultRules (
                  Id INTEGER PRIMARY KEY AUTOINCREMENT,
                  ProfileId INTEGER NOT NULL REFERENCES Profiles(Id) ON DELETE CASCADE,
                  Pattern TEXT NOT NULL,
                  Enabled INTEGER NOT NULL DEFAULT 1,
                  SortOrder INTEGER NOT NULL DEFAULT 0,
                  CreatedAt INTEGER NOT NULL,
                  UpdatedAt INTEGER NOT NULL
              );
             ",
        )?;
        if version < 4 {
            transaction.execute_batch(
                "CREATE TABLE DefaultRulesV4 (
                    Id INTEGER PRIMARY KEY AUTOINCREMENT,
                    ProfileId INTEGER REFERENCES Profiles(Id) ON DELETE CASCADE,
                    Pattern TEXT NOT NULL,
                    Enabled INTEGER NOT NULL DEFAULT 1,
                    SortOrder INTEGER NOT NULL DEFAULT 0,
                    CreatedAt INTEGER NOT NULL,
                    UpdatedAt INTEGER NOT NULL
                 );
                 INSERT INTO DefaultRulesV4 SELECT * FROM DefaultRules;
                 DROP TABLE DefaultRules;
                 ALTER TABLE DefaultRulesV4 RENAME TO DefaultRules;",
            )?;
        }
        transaction.pragma_update(None, "user_version", 4)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn schema_needs_migration(&self) -> anyhow::Result<bool> {
        let connection = self.open()?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        Ok(version < 4)
    }

    pub fn profile_count(&self) -> anyhow::Result<i64> {
        let connection = self.open()?;
        Ok(connection.query_row("SELECT COUNT(*) FROM Profiles", [], |row| row.get(0))?)
    }

    pub fn sync_detected(&self, detected: &[DetectedProfile]) -> anyhow::Result<()> {
        let mut connection = self.open()?;
        let transaction = connection.transaction()?;
        transaction.execute("UPDATE Profiles SET Available = 0", [])?;
        let now = unix_timestamp();
        let next_sort: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(SortOrder), -1) + 1 FROM Profiles",
            [],
            |row| row.get(0),
        )?;
        let mut added = 0_i64;
        for profile in detected {
            let existing_id: Option<i64> = transaction
                .query_row(
                    "SELECT Id FROM Profiles WHERE BrowserType=?1 AND ProfileId=?2",
                    params![profile.browser_type as i64, profile.profile_id],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(id) = existing_id {
                transaction.execute(
                    "UPDATE Profiles
                     SET DetectedName=?1, IconPath=COALESCE(?2, IconPath), Available=1, UpdatedAt=?3
                     WHERE Id=?4",
                    params![profile.detected_name, profile.icon_path, now, id],
                )?;
            } else {
                transaction.execute(
                    "INSERT INTO Profiles
                     (BrowserType, ProfileId, DetectedName, IconPath, SortOrder, CreatedAt, UpdatedAt)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                    params![
                        profile.browser_type as i64,
                        profile.profile_id,
                        profile.detected_name,
                        profile.icon_path,
                        next_sort + added,
                        now,
                    ],
                )?;
                added += 1;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn load_all_profiles(&self) -> anyhow::Result<Vec<Profile>> {
        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT Id, BrowserType, ProfileId, DetectedName, AliasName, Comment,
                    SortOrder, Favorite, Enabled, Available, IconPath, CreatedAt, UpdatedAt
             FROM Profiles
             ORDER BY SortOrder, Id",
        )?;
        let rows = statement.query_map([], profile_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .context("プロファイルを読み取れません")
    }

    pub fn load_selection_profiles(&self) -> anyhow::Result<Vec<Profile>> {
        let all = self.load_all_profiles()?;
        let mut favorites: Vec<_> = all
            .iter()
            .filter(|profile| profile.available && profile.enabled && profile.favorite)
            .take(5)
            .cloned()
            .collect();
        favorites.extend(
            all.into_iter()
                .filter(|profile| profile.available && profile.enabled && !profile.favorite)
                .take(20),
        );
        Ok(favorites)
    }

    pub fn save_profile(&self, profile: &Profile) -> anyhow::Result<()> {
        let connection = self.open()?;
        Self::write_profile(&connection, profile)
    }

    fn write_profile(connection: &Connection, profile: &Profile) -> anyhow::Result<()> {
        connection.execute(
            "UPDATE Profiles
             SET AliasName=?1, Comment=?2, SortOrder=?3, Favorite=?4, Enabled=?5, UpdatedAt=?6
             WHERE Id=?7",
            params![
                profile.alias_name,
                profile.comment,
                profile.sort_order,
                profile.favorite,
                profile.enabled,
                unix_timestamp(),
                profile.id
            ],
        )?;
        Ok(())
    }

    pub fn save_preferences(
        &self,
        profiles: &[Profile],
        record_url_history: bool,
        escape_hold_duration_tenths: u32,
        open_profile_on_double_click: bool,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            profiles.iter().filter(|profile| profile.favorite).count() <= 5,
            "お気に入りに登録できるプロファイルは5件までです"
        );
        anyhow::ensure!(
            (MIN_ESCAPE_HOLD_DURATION_TENTHS..=MAX_ESCAPE_HOLD_DURATION_TENTHS)
                .contains(&escape_hold_duration_tenths),
            "ESC長押し時間が範囲外です"
        );
        let mut connection = self.open()?;
        let transaction = connection.transaction()?;
        for profile in profiles {
            Self::write_profile(&transaction, profile)?;
        }
        for (key, value) in [
            (SETTING_RECORD_URL_HISTORY, u32::from(record_url_history)),
            (
                SETTING_ESCAPE_HOLD_DURATION_TENTHS,
                escape_hold_duration_tenths,
            ),
            (
                SETTING_OPEN_PROFILE_ON_DOUBLE_CLICK,
                u32::from(open_profile_on_double_click),
            ),
        ] {
            transaction.execute(
                "INSERT INTO Settings(Key, Value) VALUES (?1, ?2)
                ON CONFLICT(Key) DO UPDATE SET Value=excluded.Value",
                params![key, value.to_string()],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> anyhow::Result<Option<String>> {
        let connection = self.open()?;
        Ok(connection
            .query_row("SELECT Value FROM Settings WHERE Key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> anyhow::Result<()> {
        let connection = self.open()?;
        connection.execute(
            "INSERT INTO Settings(Key, Value) VALUES (?1, ?2)
             ON CONFLICT(Key) DO UPDATE SET Value=excluded.Value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn initial_scan_completed(&self) -> anyhow::Result<bool> {
        Ok(self
            .get_setting(SETTING_INITIAL_SCAN_COMPLETED)?
            .is_some_and(|value| value == "1"))
    }

    pub fn approved_domains(&self) -> anyhow::Result<Vec<String>> {
        let domains: Vec<String> = self
            .get_setting("risk_approved_domains")?
            .map(|json| serde_json::from_str(&json))
            .transpose()?
            .unwrap_or_default();
        crate::risk::normalize_approved_domains(&domains)
    }

    pub fn set_approved_domains(&self, domains: &[String]) -> anyhow::Result<()> {
        let domains = crate::risk::normalize_approved_domains(domains)?;
        self.set_setting("risk_approved_domains", &serde_json::to_string(&domains)?)
    }

    pub fn mark_initial_scan_completed(&self) -> anyhow::Result<()> {
        self.set_setting(SETTING_INITIAL_SCAN_COMPLETED, "1")
    }

    pub fn record_url_history_enabled(&self) -> anyhow::Result<bool> {
        Ok(self
            .get_setting(SETTING_RECORD_URL_HISTORY)?
            .is_some_and(|value| value == "1"))
    }

    pub fn set_record_url_history_enabled(&self, enabled: bool) -> anyhow::Result<()> {
        self.set_setting(SETTING_RECORD_URL_HISTORY, if enabled { "1" } else { "0" })
    }

    pub fn escape_hold_duration_tenths(&self) -> anyhow::Result<u32> {
        let Some(value) = self.get_setting(SETTING_ESCAPE_HOLD_DURATION_TENTHS)? else {
            return Ok(DEFAULT_ESCAPE_HOLD_DURATION_TENTHS);
        };
        let Ok(value) = value.parse::<u32>() else {
            return Ok(DEFAULT_ESCAPE_HOLD_DURATION_TENTHS);
        };
        if (MIN_ESCAPE_HOLD_DURATION_TENTHS..=MAX_ESCAPE_HOLD_DURATION_TENTHS).contains(&value) {
            Ok(value)
        } else {
            Ok(DEFAULT_ESCAPE_HOLD_DURATION_TENTHS)
        }
    }

    pub fn set_escape_hold_duration_tenths(&self, tenths: u32) -> anyhow::Result<()> {
        anyhow::ensure!(
            (MIN_ESCAPE_HOLD_DURATION_TENTHS..=MAX_ESCAPE_HOLD_DURATION_TENTHS).contains(&tenths),
            "ESC長押し時間は0.1秒から10.0秒の範囲で指定してください"
        );
        self.set_setting(SETTING_ESCAPE_HOLD_DURATION_TENTHS, &tenths.to_string())
    }

    pub fn open_profile_on_double_click(&self) -> anyhow::Result<bool> {
        Ok(self
            .get_setting(SETTING_OPEN_PROFILE_ON_DOUBLE_CLICK)?
            .map(|value| value != "0")
            .unwrap_or(DEFAULT_OPEN_PROFILE_ON_DOUBLE_CLICK))
    }

    pub fn set_open_profile_on_double_click(&self, enabled: bool) -> anyhow::Result<()> {
        self.set_setting(
            SETTING_OPEN_PROFILE_ON_DOUBLE_CLICK,
            if enabled { "1" } else { "0" },
        )
    }

    pub fn interruption_enabled(&self) -> anyhow::Result<bool> {
        Ok(self
            .get_setting(SETTING_INTERRUPTION_ENABLED)?
            .map(|value| value != "0")
            .unwrap_or(true))
    }

    pub fn set_interruption_enabled(&self, enabled: bool) -> anyhow::Result<()> {
        self.set_setting(
            SETTING_INTERRUPTION_ENABLED,
            if enabled { "1" } else { "0" },
        )
    }

    pub fn load_default_rules(&self) -> anyhow::Result<Vec<DefaultRule>> {
        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT Id, ProfileId, Pattern, Enabled, SortOrder, CreatedAt, UpdatedAt
             FROM DefaultRules
             ORDER BY SortOrder, Id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(DefaultRule {
                id: row.get(0)?,
                profile_id: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                pattern: row.get(2)?,
                enabled: row.get(3)?,
                sort_order: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .context("既定ルールを読み取れません")
    }

    pub fn replace_default_rules(&self, rules: &[DefaultRule]) -> anyhow::Result<()> {
        let mut connection = self.open()?;
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM DefaultRules", [])?;
        for (index, rule) in rules.iter().enumerate() {
            let stored = crate::association::StoredRule::decode(&rule.pattern)?;
            let profile_id = if stored.is_some_and(|s| s.suppress_prompt) {
                None
            } else {
                Some(rule.profile_id)
            };
            transaction.execute(
                "INSERT INTO DefaultRules
                 (ProfileId, Pattern, Enabled, SortOrder, CreatedAt, UpdatedAt)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![
                    profile_id,
                    rule.pattern,
                    rule.enabled,
                    index as i64,
                    unix_timestamp(),
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn associate_domain(&self, profile_id: i64, patterns: &[String; 2]) -> anyhow::Result<()> {
        let mut connection = self.open()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM DefaultRules WHERE Pattern=?1 OR Pattern=?2",
            params![patterns[0], patterns[1]],
        )?;
        transaction.execute("UPDATE DefaultRules SET SortOrder=SortOrder+2", [])?;
        for (order, pattern) in patterns.iter().enumerate() {
            transaction.execute(
                "INSERT INTO DefaultRules (ProfileId, Pattern, Enabled, SortOrder, CreatedAt, UpdatedAt)
                 VALUES (?1, ?2, 1, ?3, ?4, ?4)",
                params![profile_id, pattern, order as i64, unix_timestamp()],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// 同一条件・同一動作は更新する。他の規則の評価順序は変更しない。
    pub fn save_association(
        &self,
        profile_id: i64,
        rule: &crate::association::StoredRule,
    ) -> anyhow::Result<()> {
        let pattern = rule.encode()?;
        let mut rules = self.load_default_rules()?;
        let existing = rules.iter_mut().find(|entry| {
            crate::association::StoredRule::decode(&entry.pattern)
                .ok()
                .flatten()
                .is_some_and(|saved| {
                    saved.condition == rule.condition
                        && saved.suppress_prompt == rule.suppress_prompt
                })
        });
        if let Some(existing) = existing {
            existing.pattern = pattern;
            existing.profile_id = if rule.suppress_prompt { 0 } else { profile_id };
            existing.enabled = true;
        } else {
            rules.push(DefaultRule {
                id: 0,
                profile_id: if rule.suppress_prompt { 0 } else { profile_id },
                pattern,
                enabled: true,
                sort_order: rules.len() as i64,
                created_at: 0,
                updated_at: 0,
            });
        }
        self.replace_default_rules(&rules)
    }

    pub fn association_prompt_suppressed(
        &self,
        url: &crate::url_input::ValidatedUrl,
    ) -> anyhow::Result<bool> {
        Ok(self.load_default_rules()?.iter().any(|rule| {
            rule.enabled
                && crate::association::suppresses_prompt(&rule.pattern)
                && crate::rules::matches(&rule.pattern, url.original())
        }))
    }

    pub fn record_url_pattern(&self, profile_id: i64, pattern: &str) -> anyhow::Result<()> {
        let connection = self.open()?;
        connection.execute(
            "INSERT INTO UrlHistory(ProfileId, Url, OpenedAt) VALUES (?1, ?2, ?3)",
            params![profile_id, pattern, unix_timestamp()],
        )?;
        Ok(())
    }

    #[cfg(test)]
    fn url_history_count(&self) -> anyhow::Result<i64> {
        let connection = self.open()?;
        Ok(connection.query_row("SELECT COUNT(*) FROM UrlHistory", [], |row| row.get(0))?)
    }

    #[cfg(test)]
    fn latest_url_pattern(&self) -> anyhow::Result<String> {
        let connection = self.open()?;
        Ok(connection.query_row(
            "SELECT Url FROM UrlHistory ORDER BY Id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )?)
    }

    fn open(&self) -> anyhow::Result<Connection> {
        if let Some(parent) = self.database_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(&self.database_path)
            .with_context(|| format!("DBを開けません: {}", self.database_path.display()))?;
        // foreign_keys はDBファイルではなく接続ごとの設定。
        connection.pragma_update(None, "foreign_keys", true)?;
        Ok(connection)
    }
}

fn profile_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Profile> {
    let browser_value: i64 = row.get(1)?;
    let browser_type = BrowserType::from_db(browser_value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Integer, error.into())
    })?;
    Ok(Profile {
        id: row.get(0)?,
        browser_type,
        profile_id: row.get(2)?,
        detected_name: row.get(3)?,
        alias_name: row.get(4)?,
        comment: row.get(5)?,
        sort_order: row.get(6)?,
        favorite: row.get(7)?,
        enabled: row.get(8)?,
        available: row.get(9)?,
        icon_path: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_approval_is_persistent_normalized_revocable_and_rejects_wildcards() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("approval.db");
        let storage = Storage::new(&file);
        storage.initialize().unwrap();
        assert!(storage.approved_domains().unwrap().is_empty());
        storage
            .set_approved_domains(&["Microsoft-login.EXAMPLE.".into()])
            .unwrap();
        assert_eq!(
            Storage::new(&file).approved_domains().unwrap(),
            ["microsoft-login.example"]
        );
        assert!(storage.set_approved_domains(&["*.example".into()]).is_err());
        assert_eq!(
            storage.approved_domains().unwrap(),
            ["microsoft-login.example"]
        );
        storage.set_approved_domains(&[]).unwrap();
        assert!(Storage::new(&file).approved_domains().unwrap().is_empty());
    }

    #[test]
    fn preferences_are_atomic_and_reject_more_than_five_favorites() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path().join("test.db"));
        storage.initialize().unwrap();
        storage
            .sync_detected(
                &(0..6)
                    .map(|i| DetectedProfile {
                        browser_type: BrowserType::Edge,
                        profile_id: format!("Profile {i}"),
                        detected_name: format!("Profile {i}"),
                        icon_path: None,
                    })
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        let original = storage.load_all_profiles().unwrap();
        storage
            .save_preferences(&original, false, 10, true)
            .unwrap();
        let mut changed = original.clone();
        for profile in &mut changed {
            profile.favorite = true;
        }
        assert!(storage.save_preferences(&changed, true, 23, false).is_err());
        assert_eq!(storage.load_all_profiles().unwrap(), original);
        changed[5].favorite = false;
        changed[0].alias_name = "更新後".into();
        storage
            .open()
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER reject_click BEFORE UPDATE ON Settings
            WHEN NEW.Key='open_profile_on_double_click' BEGIN SELECT RAISE(ABORT, 'test'); END;",
            )
            .unwrap();
        let before = std::fs::read(storage.database_path()).unwrap();
        assert!(storage.save_preferences(&changed, true, 23, false).is_err());
        assert_eq!(storage.load_all_profiles().unwrap(), original);
        assert!(!storage.record_url_history_enabled().unwrap());
        assert_eq!(storage.escape_hold_duration_tenths().unwrap(), 10);
        assert_eq!(std::fs::read(storage.database_path()).unwrap(), before);
        storage
            .open()
            .unwrap()
            .execute_batch("DROP TRIGGER reject_click;")
            .unwrap();
        storage.save_preferences(&changed, true, 23, false).unwrap();
        assert_eq!(storage.load_all_profiles().unwrap()[0].alias_name, "更新後");
        assert!(storage.record_url_history_enabled().unwrap());
        assert_eq!(storage.escape_hold_duration_tenths().unwrap(), 23);
        assert!(!storage.open_profile_on_double_click().unwrap());
    }

    #[test]
    fn sync_preserves_user_fields_and_marks_missing_unavailable() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path().join("test.db"));
        storage.initialize().unwrap();
        storage
            .sync_detected(&[
                DetectedProfile {
                    browser_type: BrowserType::Edge,
                    profile_id: "Default".to_owned(),
                    detected_name: "Profile 1".to_owned(),
                    icon_path: None,
                },
                DetectedProfile {
                    browser_type: BrowserType::Chrome,
                    profile_id: "Profile 2".to_owned(),
                    detected_name: "Work".to_owned(),
                    icon_path: None,
                },
            ])
            .unwrap();
        let mut profile = storage.load_all_profiles().unwrap().remove(0);
        profile.alias_name = "会社".to_owned();
        profile.favorite = true;
        storage.save_profile(&profile).unwrap();

        storage
            .sync_detected(&[DetectedProfile {
                browser_type: BrowserType::Edge,
                profile_id: "Default".to_owned(),
                detected_name: "Renamed".to_owned(),
                icon_path: None,
            }])
            .unwrap();
        let profiles = storage.load_all_profiles().unwrap();
        let edge = profiles
            .iter()
            .find(|value| value.browser_type == BrowserType::Edge)
            .unwrap();
        let chrome = profiles
            .iter()
            .find(|value| value.browser_type == BrowserType::Chrome)
            .unwrap();
        assert_eq!(edge.alias_name, "会社");
        assert_eq!(edge.detected_name, "Renamed");
        assert!(edge.favorite);
        assert!(!chrome.available);
    }

    #[test]
    fn url_history_is_disabled_by_default_and_opt_in() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path().join("test.db"));
        storage.initialize().unwrap();
        assert!(!storage.record_url_history_enabled().unwrap());
        storage
            .sync_detected(&[DetectedProfile {
                browser_type: BrowserType::Edge,
                profile_id: "Default".to_owned(),
                detected_name: "Default".to_owned(),
                icon_path: None,
            }])
            .unwrap();
        let profile = storage.load_all_profiles().unwrap().remove(0);
        storage.set_record_url_history_enabled(true).unwrap();
        assert!(storage.record_url_history_enabled().unwrap());
        storage
            .record_url_pattern(profile.id, "https://example.com/*")
            .unwrap();
        assert_eq!(storage.url_history_count().unwrap(), 1);
        assert_eq!(
            storage.latest_url_pattern().unwrap(),
            "https://example.com/*"
        );
    }

    #[test]
    fn escape_hold_duration_defaults_to_one_second_and_uses_tenths() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path().join("test.db"));
        storage.initialize().unwrap();
        assert_eq!(
            storage.escape_hold_duration_tenths().unwrap(),
            DEFAULT_ESCAPE_HOLD_DURATION_TENTHS
        );
        storage.set_escape_hold_duration_tenths(11).unwrap();
        assert_eq!(storage.escape_hold_duration_tenths().unwrap(), 11);
        assert!(storage.set_escape_hold_duration_tenths(0).is_err());
        assert!(storage.set_escape_hold_duration_tenths(101).is_err());
    }

    #[test]
    fn default_rules_and_click_behavior_are_saved() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path().join("test.db"));
        storage.initialize().unwrap();
        assert!(storage.open_profile_on_double_click().unwrap());
        storage.set_open_profile_on_double_click(false).unwrap();
        assert!(!storage.open_profile_on_double_click().unwrap());
        storage
            .sync_detected(&[DetectedProfile {
                browser_type: BrowserType::Edge,
                profile_id: "Default".to_owned(),
                detected_name: "Default".to_owned(),
                icon_path: None,
            }])
            .unwrap();
        let profile_id = storage.load_all_profiles().unwrap()[0].id;

        let rule = DefaultRule {
            id: 0,
            profile_id,
            pattern: "https://example.com/*".to_owned(),
            enabled: true,
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
        };
        storage
            .replace_default_rules(std::slice::from_ref(&rule))
            .unwrap();
        let saved = storage.load_default_rules().unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].profile_id, rule.profile_id);
        assert_eq!(saved[0].pattern, rule.pattern);
        assert!(saved[0].enabled);

        let mut invalid = rule.clone();
        invalid.profile_id = profile_id + 1000;
        assert!(storage.replace_default_rules(&[rule, invalid]).is_err());
        assert_eq!(storage.load_default_rules().unwrap(), saved);
    }

    #[test]
    fn interruption_setting_and_domain_association_are_persistent() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path().join("test.db"));
        storage.initialize().unwrap();
        assert!(storage.interruption_enabled().unwrap());
        storage.set_interruption_enabled(false).unwrap();
        assert!(!storage.interruption_enabled().unwrap());
        storage
            .sync_detected(&[
                DetectedProfile {
                    browser_type: BrowserType::Edge,
                    profile_id: "Default".into(),
                    detected_name: "Edge".into(),
                    icon_path: None,
                },
                DetectedProfile {
                    browser_type: BrowserType::Chrome,
                    profile_id: "Default".into(),
                    detected_name: "Chrome".into(),
                    icon_path: None,
                },
            ])
            .unwrap();
        let profiles = storage.load_all_profiles().unwrap();
        let patterns = [
            "http://example.com/*".into(),
            "https://example.com/*".into(),
        ];
        storage.associate_domain(profiles[0].id, &patterns).unwrap();
        storage.associate_domain(profiles[1].id, &patterns).unwrap();
        let rules = storage.load_default_rules().unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].pattern, patterns[0]);
        assert_eq!(rules[1].pattern, patterns[1]);
        assert!(rules.iter().all(|rule| rule.profile_id == profiles[1].id));
    }

    #[test]
    fn migration_preserves_profiles_preferences_and_is_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path().join("test.db"));
        storage.initialize().unwrap();
        storage
            .sync_detected(&[DetectedProfile {
                browser_type: BrowserType::Edge,
                profile_id: "Default".into(),
                detected_name: "移行前".into(),
                icon_path: None,
            }])
            .unwrap();
        let mut profile = storage.load_all_profiles().unwrap()[0].clone();
        profile.alias_name = "表示名".into();
        profile.favorite = true;
        storage.save_profile(&profile).unwrap();
        let profiles = storage.load_all_profiles().unwrap();
        storage.set_escape_hold_duration_tenths(27).unwrap();
        storage.set_open_profile_on_double_click(false).unwrap();
        let connection = storage.open().unwrap();
        connection
            .execute_batch("DROP TABLE DefaultRules; PRAGMA user_version=2;")
            .unwrap();
        drop(connection);
        assert!(storage.schema_needs_migration().unwrap());
        storage.initialize().unwrap();
        storage.initialize().unwrap();
        assert!(!storage.schema_needs_migration().unwrap());
        assert_eq!(storage.escape_hold_duration_tenths().unwrap(), 27);
        assert!(!storage.open_profile_on_double_click().unwrap());
        assert!(storage.load_default_rules().unwrap().is_empty());
        assert_eq!(storage.load_all_profiles().unwrap(), profiles);
    }

    #[test]
    fn structured_rules_upsert_preserve_order_and_suppression_has_no_profile_dependency() {
        use crate::association::{StoredRule, UrlParts};
        use crate::url_input::ValidatedUrl;
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path().join("test.db"));
        storage.initialize().unwrap();
        storage
            .sync_detected(&[DetectedProfile {
                browser_type: BrowserType::Chrome,
                profile_id: "Default".into(),
                detected_name: "個人".into(),
                icon_path: None,
            }])
            .unwrap();
        let profile = storage.load_all_profiles().unwrap()[0].id;
        let url =
            ValidatedUrl::parse("https://apps.powerapps.com/play?tenantId=123&private=secret")
                .unwrap();
        let parts = UrlParts::new(&url);
        let mut selection = parts.defaults();
        selection.queries[0] = true;
        let route = StoredRule::new(parts.condition(&selection).unwrap(), false);
        storage.save_association(profile, &route).unwrap();
        storage.save_association(profile, &route).unwrap();
        let skip = StoredRule::new(parts.condition(&selection).unwrap(), true);
        storage.save_association(profile, &skip).unwrap();
        let rules = storage.load_default_rules().unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].pattern, route.encode().unwrap());
        assert_eq!(rules[1].profile_id, 0);
        assert!(storage.association_prompt_suppressed(&url).unwrap());
        storage
            .open()
            .unwrap()
            .execute("DELETE FROM Profiles WHERE Id=?1", [profile])
            .unwrap();
        assert_eq!(storage.load_default_rules().unwrap().len(), 1);
        assert!(storage.association_prompt_suppressed(&url).unwrap());
        assert!(
            !String::from_utf8_lossy(&std::fs::read(storage.database_path()).unwrap())
                .contains("secret")
        );
    }

    #[test]
    fn schema_three_upgrade_preserves_rule_ids_patterns_and_order() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path().join("test.db"));
        storage.initialize().unwrap();
        storage
            .sync_detected(&[DetectedProfile {
                browser_type: BrowserType::Chrome,
                profile_id: "Default".into(),
                detected_name: "個人".into(),
                icon_path: None,
            }])
            .unwrap();
        let profile = storage.load_all_profiles().unwrap()[0].id;
        storage
            .associate_domain(
                profile,
                &[
                    "http://example.com/*".into(),
                    "https://example.com/*".into(),
                ],
            )
            .unwrap();
        let before = storage.load_default_rules().unwrap();
        storage
            .open()
            .unwrap()
            .pragma_update(None, "user_version", 3)
            .unwrap();
        storage.initialize().unwrap();
        storage.initialize().unwrap();
        assert_eq!(before, storage.load_default_rules().unwrap());
        assert!(!storage.schema_needs_migration().unwrap());
    }

    #[test]
    fn newer_schema_is_rejected_without_modifying_the_database() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path().join("test.db"));
        storage.initialize().unwrap();
        storage
            .open()
            .unwrap()
            .pragma_update(None, "user_version", 5)
            .unwrap();
        let before = std::fs::read(storage.database_path()).unwrap();
        assert!(storage.initialize().is_err());
        assert_eq!(std::fs::read(storage.database_path()).unwrap(), before);
    }
}
