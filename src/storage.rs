use crate::model::{BrowserType, DetectedProfile, Profile};
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
pub const DEFAULT_ESCAPE_HOLD_DURATION_TENTHS: u32 = 10;
pub const MIN_ESCAPE_HOLD_DURATION_TENTHS: u32 = 1;
pub const MAX_ESCAPE_HOLD_DURATION_TENTHS: u32 = 100;

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
        let connection = self.open()?;
        connection.execute_batch(
            "PRAGMA journal_mode=DELETE;
             PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS Profiles (
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
             PRAGMA user_version=2;",
        )?;
        Ok(())
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
        Connection::open(&self.database_path)
            .with_context(|| format!("DBを開けません: {}", self.database_path.display()))
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
}
