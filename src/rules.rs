use crate::url_input::ValidatedUrl;
use url::{Position, Url};

pub fn site_pattern(url: &ValidatedUrl) -> String {
    let parsed = Url::parse(url.original()).expect("ValidatedUrl must contain a valid URL");
    format!("{}://{}/*", parsed.scheme(), host_with_port(&parsed))
}

pub fn domain_patterns(url: &ValidatedUrl) -> [String; 2] {
    let parsed = Url::parse(url.original()).expect("ValidatedUrl must contain a valid URL");
    let host = host_with_port(&parsed);
    [format!("http://{host}/*"), format!("https://{host}/*")]
}

pub fn leading_path_pattern(url: &ValidatedUrl) -> String {
    let parsed = Url::parse(url.original()).expect("ValidatedUrl must contain a valid URL");
    let first_segment = parsed
        .path_segments()
        .and_then(|mut segments| segments.find(|segment| !segment.is_empty()));
    let path = first_segment
        .map(|segment| format!("/{segment}"))
        .unwrap_or_default();
    format!(
        "{}://{}{}/*",
        parsed.scheme(),
        host_with_port(&parsed),
        path
    )
}

pub fn wildcard_pattern(url: &ValidatedUrl) -> String {
    let parsed = Url::parse(url.original()).expect("ValidatedUrl must contain a valid URL");
    // 認証情報は全体を伏せ、同じホストの元URLに一致する区切りだけを残す。
    let user_info = if !parsed.username().is_empty() || parsed.password().is_some() {
        "*@"
    } else {
        ""
    };
    let mut pattern = format!(
        "{}://{user_info}{}",
        parsed.scheme(),
        host_with_port(&parsed)
    );

    // 末尾の非空パス要素を伏せる。単一要素のパスや連続・末尾スラッシュも扱う。
    let path = parsed.path();
    let trimmed = path.trim_end_matches('/');
    if let Some(last_slash) = trimmed.rfind('/') {
        pattern.push_str(&path[..=last_slash]);
        pattern.push('*');
        pattern.push_str(&path[trimmed.len()..]);
    } else {
        pattern.push_str(path);
    }

    if let Some(query) = parsed.query() {
        pattern.push('?');
        for (index, part) in query.split('&').enumerate() {
            if index > 0 {
                pattern.push('&');
            }
            if let Some((name, _)) = part.split_once('=') {
                pattern.push_str(name);
                pattern.push('=');
            }
            // 名前のない値や、=のないトークンもそのまま保存しない。
            pattern.push('*');
        }
    }
    if parsed.fragment().is_some() {
        pattern.push_str("#*");
    }
    pattern
}

pub fn matches(pattern: &str, value: &str) -> bool {
    // URL形式のルールはホストとパスを別々に照合し、ホストの * が / を越えないようにする。
    // 正規化は判定だけに使い、起動へ渡す元URLは変更しない。
    if let (Ok(pattern_url), Ok(value_url)) = (Url::parse(pattern), Url::parse(value))
        && matches!(pattern_url.scheme(), "http" | "https")
        && pattern_url.host_str().is_some()
    {
        return wildcard_matches(
            &pattern_url[..Position::BeforePath],
            &value_url[..Position::BeforePath],
        ) && path_matches(
            &pattern_url[Position::BeforePath..],
            &value_url[Position::BeforePath..],
        );
    }
    path_matches(pattern, value)
}

fn path_matches(pattern: &str, value: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix("/*")
        && let Some(tail) = value.strip_prefix(prefix)
        && (tail.is_empty() || tail.starts_with('?') || tail.starts_with('#'))
    {
        return true;
    }
    wildcard_matches(pattern, value)
}

fn wildcard_matches(pattern: &str, value: &str) -> bool {
    let pattern: Vec<_> = pattern.chars().collect();
    let value: Vec<_> = value.chars().collect();
    let mut pattern_index = 0;
    let mut value_index = 0;
    let mut star_index = None;
    let mut star_value_index = 0;

    while value_index < value.len() {
        if pattern_index < pattern.len() && pattern[pattern_index] == '*' {
            star_index = Some(pattern_index);
            pattern_index += 1;
            star_value_index = value_index;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == value[value_index] {
            pattern_index += 1;
            value_index += 1;
        } else if let Some(star) = star_index {
            pattern_index = star + 1;
            star_value_index += 1;
            value_index = star_value_index;
        } else {
            return false;
        }
    }

    while pattern_index < pattern.len() && pattern[pattern_index] == '*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

fn host_with_port(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_site_and_leading_path_patterns() {
        let url = ValidatedUrl::parse(
            "https://portal.example.com/team/invoices/9382?tenant=acme#details",
        )
        .unwrap();
        assert_eq!(site_pattern(&url), "https://portal.example.com/*");
        assert_eq!(
            domain_patterns(&url),
            [
                "http://portal.example.com/*".to_owned(),
                "https://portal.example.com/*".to_owned()
            ]
        );
        assert_eq!(
            leading_path_pattern(&url),
            "https://portal.example.com/team/*"
        );
        assert_eq!(
            wildcard_pattern(&url),
            "https://portal.example.com/team/invoices/*?tenant=*#*"
        );
    }

    #[test]
    fn redacted_patterns_hide_values_and_still_match_the_original_url() {
        for (source, expected) in [
            (
                "HTTPS://alice:password@EXAMPLE.COM:443/team/private-item?tenant=acme&token=secret#private-fragment",
                "https://*@example.com/team/*?tenant=*&token=*#*",
            ),
            (
                "http://user:password@[::1]:8080/private-item?=secret&bare-token&access=one%26two&access=three#private-fragment",
                "http://*@[::1]:8080/*?=*&*&access=*&access=*#*",
            ),
            (
                "https://example.com/team//private-item///?bare-token",
                "https://example.com/team//*///?*",
            ),
            (
                "https://example.com?token=secret",
                "https://example.com/?token=*",
            ),
            ("https://example.com/", "https://example.com/"),
            ("https://example.com/?#", "https://example.com/?*#*"),
            (
                "https://example.com/%E7%A7%98%E5%AF%86",
                "https://example.com/*",
            ),
        ] {
            let url = ValidatedUrl::parse(source).unwrap();
            let pattern = wildcard_pattern(&url);
            assert_eq!(pattern, expected);
            assert!(matches(&pattern, source), "{pattern}");
            assert!(!matches(
                &pattern,
                "https://other.example.net/team/private-item?token=secret"
            ));
            assert_eq!(url.original(), source);
        }
    }

    #[test]
    fn matches_only_star_as_a_wildcard() {
        assert!(matches("https://example.com/*", "https://example.com/a/b"));
        assert!(matches("https://example.com/*", "https://example.com"));
        assert!(matches(
            "https://*/team/*",
            "https://portal.example.com/team/a"
        ));
        assert!(!matches(
            "https://example.com/*",
            "https://other.example.com/a"
        ));
        assert!(!matches("https://example.com/a", "https://example.com/ab"));
    }

    #[test]
    fn generated_rules_match_their_source_and_respect_path_boundaries() {
        for source in [
            "https://example.com?x=1#part",
            "HTTPS://EXAMPLE.COM:443/team?x=1#part",
            "https://example.com/team#part",
            "https://example.com/開発/item",
            "http://[::1]:8080/team?x=1",
        ] {
            let url = ValidatedUrl::parse(source).unwrap();
            assert!(matches(&site_pattern(&url), source), "site: {source}");
            assert!(
                matches(&leading_path_pattern(&url), source),
                "path: {source}"
            );
        }
        assert!(!matches(
            "https://example.com/team/*",
            "https://example.com/teams?a=1"
        ));
        assert!(!matches(
            "https://example.com/Team/*",
            "https://example.com/team/a"
        ));
    }

    #[test]
    fn host_wildcard_cannot_match_a_different_host_via_its_path() {
        assert!(matches(
            "https://*.example.com/*",
            "https://portal.example.com/a"
        ));
        assert!(!matches(
            "https://*.example.com/*",
            "https://evil.invalid/path/.example.com/a"
        ));
    }

    #[test]
    fn wildcard_also_matches_an_asterisk_in_the_input() {
        assert!(matches("*a", "*ba"));
        assert!(matches("https://example.com/*a", "https://example.com/*ba"));
    }
}
