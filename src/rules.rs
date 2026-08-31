use crate::url_input::ValidatedUrl;
use url::Url;

pub fn site_pattern(url: &ValidatedUrl) -> String {
    let parsed = Url::parse(url.original()).expect("ValidatedUrl must contain a valid URL");
    format!("{}://{}/*", parsed.scheme(), host_with_port(&parsed))
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
    url.original().to_owned()
}

pub fn matches(pattern: &str, value: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix("/*")
        && value == prefix
    {
        return true;
    }
    let pattern: Vec<_> = pattern.chars().collect();
    let value: Vec<_> = value.chars().collect();
    let mut pattern_index = 0;
    let mut value_index = 0;
    let mut star_index = None;
    let mut star_value_index = 0;

    while value_index < value.len() {
        if pattern_index < pattern.len() && pattern[pattern_index] == value[value_index] {
            pattern_index += 1;
            value_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == '*' {
            star_index = Some(pattern_index);
            pattern_index += 1;
            star_value_index = value_index;
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
            leading_path_pattern(&url),
            "https://portal.example.com/team/*"
        );
        assert_eq!(wildcard_pattern(&url), url.original());
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
}
