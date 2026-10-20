//! 入れ子URLは照合専用。一段ずつ復号し、元URLの起動・Safe Linksの検査を迂回しない。
use crate::url_input::ValidatedUrl;
use url::Url;

pub const MAX_DEPTH: usize = 3;
pub const MAX_CHILDREN: usize = 16;
pub const MAX_LENGTH: usize = 32 * 1024;

pub fn child(url: &Url, key: &str) -> Option<ValidatedUrl> {
    let mut values = url.query_pairs().filter(|(name, _)| name == key);
    let (_, value) = values.next()?;
    if values.next().is_some() || value.len() > MAX_LENGTH {
        return None;
    }
    let input = ValidatedUrl::parse(value.into_owned()).ok()?;
    let parsed = Url::parse(input.original()).ok()?;
    (parsed.username().is_empty() && parsed.password().is_none()).then_some(input)
}

pub fn children(url: &Url) -> Vec<(String, ValidatedUrl)> {
    if url.as_str().len() > MAX_LENGTH {
        return vec![];
    }
    url.query_pairs()
        .take(MAX_CHILDREN)
        .filter_map(|(name, _)| child(url, &name).map(|value| (name.into_owned(), value)))
        .collect()
}

pub fn is_teams_wrapper(url: &ValidatedUrl) -> bool {
    Url::parse(url.original()).is_ok_and(|url| {
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.port_or_known_default() == Some(443)
            && url.host_str() == Some("teams.public.onecdn.static.microsoft")
            && url.path() == "/evergreen-assets/safelinks/2/atp-safelinks.html"
    })
}

pub fn teams_target(url: &ValidatedUrl) -> Option<ValidatedUrl> {
    if !is_teams_wrapper(url) {
        return None;
    }
    child(&Url::parse(url.original()).ok()?, "url")
}

pub fn teams_account(url: &ValidatedUrl) -> Option<(String, String)> {
    let target = teams_target(url)?;
    let target = Url::parse(target.original()).ok()?;
    let host = target.host_str()?;
    if target.scheme() != "https"
        || target.port_or_known_default() != Some(443)
        || !host.ends_with(".sharepoint.com")
    {
        return None;
    }
    let mut values = target.query_pairs().filter(|(key, _)| key == "TeamsCID");
    let (_, value) = values.next()?;
    if values.next().is_some() || !valid_guid(&value) {
        return None;
    }
    Some((host.into(), value.into_owned()))
}

fn valid_guid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

/// 表示だけを短縮する。元の条件値を変更しない。
pub fn preview(value: &str) -> String {
    let mut chars = value.chars();
    let short: String = chars.by_ref().take(110).collect();
    if chars.next().is_some() {
        format!("{short}…")
    } else {
        short
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub fn wrapper(inner: &str) -> ValidatedUrl {
        let mut url = Url::parse("https://teams.public.onecdn.static.microsoft/evergreen-assets/safelinks/2/atp-safelinks.html").unwrap();
        url.query_pairs_mut()
            .append_pair("url", inner)
            .append_pair("pc", "synthetic-only");
        ValidatedUrl::parse(url.to_string()).unwrap()
    }
    #[test]
    fn decoding_is_one_level_and_duplicates_fail_closed() {
        let url = wrapper(
            "https://tenant.sharepoint.com/a%20b?TeamsCID=11111111-2222-3333-4444-555555555555",
        );
        assert_eq!(
            teams_target(&url).unwrap().original(),
            "https://tenant.sharepoint.com/a%20b?TeamsCID=11111111-2222-3333-4444-555555555555"
        );
        assert!(teams_account(&url).is_some());
        assert!(
            teams_target(
                &ValidatedUrl::parse(format!("{}&url=https%3A%2F%2Fevil.example", url.original()))
                    .unwrap()
            )
            .is_none()
        );
        assert!(teams_account(&wrapper("https://tenant.sharepoint.com/a?TeamsCID=x")).is_none());
        assert!(teams_account(&wrapper("https://tenant.sharepoint.com.evil.example/a?TeamsCID=11111111-2222-3333-4444-555555555555")).is_none());
        assert!(teams_account(&wrapper("https://tenant.sharepoint.com/a?TeamsCID=11111111-2222-3333-4444-555555555555&TeamsCID=11111111-2222-3333-4444-555555555555")).is_none());
        assert!(
            teams_target(
                &ValidatedUrl::parse(
                    url.original()
                        .replace("static.microsoft", "static.microsoft.evil.example")
                )
                .unwrap()
            )
            .is_none()
        );
        assert_eq!(url.original().matches("synthetic-only").count(), 1);
    }
}
