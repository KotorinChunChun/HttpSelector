//! URL部分選択の永続形式。生成済みの式をANDで評価し、クエリ順序に依存させない。
use crate::url_input::ValidatedUrl;
use anyhow::{Context, ensure};
use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use url::{Position, Url};

const PREFIX: &str = "urlparts-v1:";
const GENERATOR_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UrlParts {
    pub host: String,
    pub paths: Vec<String>,
    /// エンコード済みの名前と値。`=`なしと空値を区別する。
    pub queries: Vec<String>,
}

impl UrlParts {
    pub fn new(url: &ValidatedUrl) -> Self {
        let parsed = Url::parse(url.original()).expect("validated URL");
        Self {
            host: parsed[Position::BeforeHost..Position::AfterPort].to_owned(),
            paths: parsed
                .path()
                .strip_prefix('/')
                .unwrap_or(parsed.path())
                .split('/')
                .map(str::to_owned)
                .collect(),
            queries: parsed
                .query()
                .filter(|s| !s.is_empty())
                .map(|s| s.split('&').map(str::to_owned).collect())
                .unwrap_or_default(),
        }
    }

    pub fn defaults(&self) -> Selection {
        Selection {
            host: true,
            paths: vec![false; self.paths.len()],
            queries: vec![false; self.queries.len()],
        }
    }

    pub fn condition(&self, selection: &Selection) -> anyhow::Result<Condition> {
        ensure!(
            selection.paths.len() == self.paths.len()
                && selection.queries.len() == self.queries.len(),
            "URLの選択状態が不正です"
        );
        let last = selection.paths.iter().rposition(|on| *on);
        let paths = last
            .map(|last| {
                self.paths[..=last]
                    .iter()
                    .zip(&selection.paths)
                    .map(|(value, on)| on.then(|| value.clone()))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Condition {
            host: selection.host.then(|| self.host.clone()),
            paths,
            descendants: last.is_none_or(|last| last + 1 < self.paths.len()),
            queries: self
                .queries
                .iter()
                .zip(&selection.queries)
                .filter_map(|(value, on)| on.then(|| value.clone()))
                .collect(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Selection {
    pub host: bool,
    pub paths: Vec<bool>,
    pub queries: Vec<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    pub host: Option<String>,
    pub paths: Vec<Option<String>>,
    pub descendants: bool,
    pub queries: Vec<String>,
}

impl Condition {
    pub fn regexes(&self) -> Vec<String> {
        let mut body = format!(
            "\\Ahttps?://{}",
            self.host
                .as_ref()
                .map(|s| regex::escape(s))
                .unwrap_or_else(|| "[^/?#@]+".into())
        );
        for path in &self.paths {
            body.push('/');
            body.push_str(
                &path
                    .as_ref()
                    .map(|s| regex::escape(s))
                    .unwrap_or_else(|| "[^/?#]*".into()),
            );
        }
        if self.descendants {
            body.push_str("(?:/[^?#]*)?");
        }
        body.push_str("(?:\\?[^#]*)?(?:#.*)?\\z");
        let mut expressions = vec![body];
        for pair in &self.queries {
            expressions.push(format!(
                "\\Ahttps?://[^?#]*\\?(?:[^&#]*&)*{}(?:&[^#]*)?(?:#.*)?\\z",
                regex::escape(pair)
            ));
        }
        expressions
    }

    pub fn summary(&self) -> String {
        let host = self.host.as_deref().unwrap_or("*");
        let paths: String = self
            .paths
            .iter()
            .map(|p| format!("/{}", p.as_deref().unwrap_or("*")))
            .collect();
        let tail = if self.descendants { "/*" } else { "" };
        let query = if self.queries.is_empty() {
            String::new()
        } else {
            format!("?{}", self.queries.join("&"))
        };
        format!("http(s)://{host}{paths}{tail}{query}")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoredRule {
    pub generator_version: u32,
    pub condition: Condition,
    pub regexes: Vec<String>,
    pub suppress_prompt: bool,
}

impl StoredRule {
    pub fn new(condition: Condition, suppress_prompt: bool) -> Self {
        let regexes = condition.regexes();
        Self {
            generator_version: GENERATOR_VERSION,
            condition,
            regexes,
            suppress_prompt,
        }
    }

    pub fn encode(&self) -> anyhow::Result<String> {
        self.validate()?;
        Ok(format!("{PREFIX}{}", serde_json::to_string(self)?))
    }

    pub fn decode(pattern: &str) -> anyhow::Result<Option<Self>> {
        let Some(json) = pattern.strip_prefix(PREFIX) else {
            return Ok(None);
        };
        let value: Self = serde_json::from_str(json).context("URL部分ルールを読み取れません")?;
        value.validate()?;
        Ok(Some(value))
    }

    fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            self.generator_version == GENERATOR_VERSION,
            "対応していないURL部分ルールです"
        );
        ensure!(
            self.regexes == self.condition.regexes(),
            "保存された式と条件が一致しません"
        );
        for expression in &self.regexes {
            compile(expression)?;
        }
        Ok(())
    }

    pub fn matches(&self, value: &str) -> bool {
        let Ok(mut url) = Url::parse(value) else {
            return false;
        };
        if !matches!(url.scheme(), "http" | "https") {
            return false;
        }
        // 認証情報は照合・保存に含めない。起動には元のValidatedUrlをそのまま使う。
        let _ = url.set_username("");
        let _ = url.set_password(None);
        self.regexes
            .iter()
            .all(|s| compile(s).is_ok_and(|regex| regex.is_match(url.as_str())))
    }
}

fn compile(expression: &str) -> anyhow::Result<regex::Regex> {
    ensure!(expression.len() <= 128 * 1024, "正規表現が長すぎます");
    Ok(RegexBuilder::new(expression)
        .size_limit(2 * 1024 * 1024)
        .build()?)
}

pub fn is_structured(pattern: &str) -> bool {
    pattern.starts_with(PREFIX)
}

pub fn suppresses_prompt(pattern: &str) -> bool {
    StoredRule::decode(pattern)
        .ok()
        .flatten()
        .is_some_and(|s| s.suppress_prompt)
}

pub fn display_pattern(pattern: &str) -> String {
    match StoredRule::decode(pattern) {
        Ok(Some(rule)) => rule.condition.summary(),
        Ok(None) => pattern.to_owned(),
        Err(_) => "（無効なURL部分ルール）".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> ValidatedUrl {
        ValidatedUrl::parse(
            "https://apps.powerapps.com/play/e/tenant/a/app?tenantId=123&mode=edit&itemId=5#secret",
        )
        .unwrap()
    }

    #[test]
    fn default_only_fixes_host_and_never_saves_unselected_values() {
        let parts = UrlParts::new(&sample());
        let stored = StoredRule::new(parts.condition(&parts.defaults()).unwrap(), false);
        let text = stored.encode().unwrap();
        for private in ["tenantId", "itemId", "secret", "/play", "mode"] {
            assert!(!text.contains(private));
        }
        assert!(stored.matches("http://APPS.POWERAPPS.COM/other?x=1#any"));
        assert!(!stored.matches("https://apps.powerapps.com.evil.invalid/"));
        assert_eq!(StoredRule::decode(&text).unwrap(), Some(stored));
    }

    #[test]
    fn selected_queries_are_exact_order_independent_and_do_not_cross_fragment() {
        let parts = UrlParts::new(&sample());
        let mut selection = parts.defaults();
        selection.queries = vec![true, false, true];
        let rule = StoredRule::new(parts.condition(&selection).unwrap(), false);
        for value in [
            "http://apps.powerapps.com/?tenantId=123&itemId=5",
            "https://apps.powerapps.com/?itemId=5&other=2&tenantId=123#abc",
        ] {
            assert!(rule.matches(value), "{value}");
        }
        for query in [
            "tenantId=123&itemId=55",
            "xTenantId=123&itemId=5",
            "tenantId=123",
            "tenantId=123#itemId=5",
            "tenantId=12&itemId=5",
        ] {
            assert!(
                !rule.matches(&format!("https://apps.powerapps.com/?{query}")),
                "{query}"
            );
        }
    }

    #[test]
    fn path_positions_literals_and_descendants_are_preserved() {
        let parts = UrlParts::new(&sample());
        let mut selection = parts.defaults();
        selection.paths[2] = true;
        let rule = StoredRule::new(parts.condition(&selection).unwrap(), false);
        assert!(rule.matches("https://apps.powerapps.com/other/e/tenant/any/depth"));
        assert!(!rule.matches("https://apps.powerapps.com/tenant/any"));
        selection.paths[4] = true;
        let rule = StoredRule::new(parts.condition(&selection).unwrap(), false);
        assert!(rule.matches(sample().original()));
        assert!(!rule.matches("https://apps.powerapps.com/play/e/tenant/a/app/more"));
    }

    #[test]
    fn encoded_delimiters_duplicates_empty_values_and_bare_keys() {
        let url = ValidatedUrl::parse(
            "https://user:secret@example.com/a//file.html?k=a%26b&k=two&empty=&bare#secret",
        )
        .unwrap();
        let parts = UrlParts::new(&url);
        let mut selection = parts.defaults();
        selection.queries = vec![true, true, true, true];
        selection.paths[2] = true;
        let rule = StoredRule::new(parts.condition(&selection).unwrap(), true);
        assert!(rule.matches(url.original()));
        assert!(rule.matches("http://example.com/x//file.html?bare&empty=&k=two&k=a%26b"));
        assert!(!rule.matches("http://example.com/x//fileXhtml?bare&empty=&k=two&k=a%26b"));
        assert!(!rule.matches("http://example.com/x//file.html?bare=&empty=&k=two&k=a%26b"));
        assert!(!rule.encode().unwrap().contains("user"));
    }

    #[test]
    fn ipv6_nonstandard_ports_and_all_off() {
        let parts = UrlParts::new(&ValidatedUrl::parse("https://[::1]:8443/a").unwrap());
        let mut selection = parts.defaults();
        let rule = StoredRule::new(parts.condition(&selection).unwrap(), false);
        assert!(rule.matches("http://[::1]:8443/b"));
        assert!(!rule.matches("http://[::1]/b"));
        selection.host = false;
        let rule = StoredRule::new(parts.condition(&selection).unwrap(), false);
        assert!(rule.matches("https://other.example/anything?x=2#x"));
        assert!(!rule.matches("ftp://other.example/a"));
    }
}
