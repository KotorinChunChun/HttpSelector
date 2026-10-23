use anyhow::{Context, ensure};
use percent_encoding::percent_decode_str;
use url::Url;

pub const MAX_URL_BYTES: usize = 32 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedUrl {
    original: String,
    decoded_preview: String,
    force_confirmation: bool,
}

impl ValidatedUrl {
    pub fn parse(value: impl Into<String>) -> anyhow::Result<Self> {
        let original = value.into();
        ensure!(!original.is_empty(), "URLが指定されていません");
        ensure!(
            original.len() <= MAX_URL_BYTES,
            "URLが長すぎます（上限: {MAX_URL_BYTES} bytes）"
        );
        ensure!(
            !original.chars().any(char::is_control),
            "URLに制御文字が含まれています"
        );

        let parsed = Url::parse(&original).context("URLの形式が正しくありません")?;
        ensure!(
            matches!(parsed.scheme(), "http" | "https"),
            "http / https 以外のURLは開けません"
        );
        ensure!(parsed.host_str().is_some(), "URLにホスト名がありません");

        let decoded = percent_decode_str(&original).decode_utf8_lossy();
        let decoded_preview = decoded
            .chars()
            .flat_map(|character| match character {
                '\r' => "\\r".chars().collect::<Vec<_>>(),
                '\n' => "\\n".chars().collect::<Vec<_>>(),
                '\t' => "\\t".chars().collect::<Vec<_>>(),
                other if other.is_control() => {
                    format!("\\u{{{:04X}}}", other as u32).chars().collect()
                }
                other => vec![other],
            })
            .collect();

        Ok(Self {
            original,
            decoded_preview,
            force_confirmation: false,
        })
    }

    pub fn original(&self) -> &str {
        &self.original
    }

    pub fn with_forced_confirmation(mut self, enabled: bool) -> Self {
        self.force_confirmation = enabled;
        self
    }

    pub fn force_confirmation(&self) -> bool {
        self.force_confirmation
    }

    pub(crate) fn copy_data_kind(&self) -> usize {
        if self.force_confirmation { 3 } else { 1 }
    }

    pub fn decoded_preview(&self) -> &str {
        &self.decoded_preview
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_http_and_builds_preview() {
        let url = ValidatedUrl::parse("https://example.com/path?a=b%20c").unwrap();
        assert_eq!(url.original(), "https://example.com/path?a=b%20c");
        assert_eq!(url.decoded_preview(), "https://example.com/path?a=b c");
    }

    #[test]
    fn rejects_non_web_protocols() {
        assert!(ValidatedUrl::parse("file:///C:/Windows/win.ini").is_err());
        assert!(ValidatedUrl::parse("javascript:alert(1)").is_err());
    }

    #[test]
    fn rejects_control_characters_in_original() {
        assert!(ValidatedUrl::parse("https://example.com/\nnext").is_err());
    }

    #[test]
    fn makes_decoded_control_characters_visible() {
        let url = ValidatedUrl::parse("https://example.com/%0Aadmin").unwrap();
        assert_eq!(url.decoded_preview(), "https://example.com/\\nadmin");
    }
}
