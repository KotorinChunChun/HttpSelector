use std::fmt;
use std::str::FromStr;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(i64)]
pub enum BrowserType {
    Edge = 1,
    Chrome = 2,
    Firefox = 3,
}

impl BrowserType {
    pub const ALL: [Self; 3] = [Self::Edge, Self::Chrome, Self::Firefox];

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Edge => "Microsoft Edge",
            Self::Chrome => "Google Chrome",
            Self::Firefox => "Mozilla Firefox",
        }
    }

    pub const fn icon_glyph(self) -> &'static str {
        match self {
            Self::Edge => "🔵",
            Self::Chrome => "🟢",
            Self::Firefox => "🟠",
        }
    }

    pub fn from_db(value: i64) -> anyhow::Result<Self> {
        match value {
            1 => Ok(Self::Edge),
            2 => Ok(Self::Chrome),
            3 => Ok(Self::Firefox),
            _ => anyhow::bail!("unknown browser type: {value}"),
        }
    }
}

impl fmt::Display for BrowserType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.display_name())
    }
}

impl FromStr for BrowserType {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "edge" => Ok(Self::Edge),
            "chrome" => Ok(Self::Chrome),
            "firefox" => Ok(Self::Firefox),
            _ => anyhow::bail!("unknown browser: {value}"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Profile {
    pub id: i64,
    pub browser_type: BrowserType,
    pub profile_id: String,
    pub detected_name: String,
    pub alias_name: String,
    pub comment: String,
    pub sort_order: i64,
    pub favorite: bool,
    pub enabled: bool,
    pub available: bool,
    pub icon_path: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Profile {
    pub fn display_name(&self) -> &str {
        if self.alias_name.trim().is_empty() {
            &self.detected_name
        } else {
            &self.alias_name
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DetectedProfile {
    pub browser_type: BrowserType,
    pub profile_id: String,
    pub detected_name: String,
    pub icon_path: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DefaultRule {
    pub id: i64,
    pub profile_id: i64,
    pub pattern: String,
    pub enabled: bool,
    pub sort_order: i64,
    pub created_at: i64,
    pub updated_at: i64,
}
