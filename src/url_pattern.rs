use crate::url_input::ValidatedUrl;
use url::Url;

/// Builds a privacy-preserving, human-readable pattern for optional local URL history.
/// Folder context is retained while the final path component, parameter values, and
/// fragment are replaced with `*`.
pub fn from_url(url: &ValidatedUrl) -> String {
    let parsed = Url::parse(url.original()).expect("ValidatedUrl must contain a valid URL");
    let host = parsed.host_str().unwrap_or_default();
    let mut value = format!("{}://{}", parsed.scheme(), host);
    if let Some(port) = parsed.port() {
        value.push(':');
        value.push_str(&port.to_string());
    }

    let segments = parsed
        .path_segments()
        .map(|parts| parts.filter(|part| !part.is_empty()).collect::<Vec<_>>())
        .unwrap_or_default();
    if segments.len() > 1 {
        for segment in &segments[..segments.len() - 1] {
            value.push('/');
            value.push_str(segment);
        }
    } else if let Some(segment) = segments.first() {
        value.push('/');
        value.push_str(segment);
    }
    value.push_str("/*");

    if let Some(query) = parsed.query() {
        let names = query
            .split('&')
            .filter_map(|part| part.split_once('=').map(|(name, _)| name))
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>();
        if names.is_empty() {
            value.push_str("?*");
        } else {
            value.push('?');
            value.push_str(
                &names
                    .into_iter()
                    .map(|name| format!("{name}=*"))
                    .collect::<Vec<_>>()
                    .join("&"),
            );
        }
    }
    if parsed.fragment().is_some() {
        value.push_str("#*");
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_variable_url_parts_with_wildcards() {
        let url = ValidatedUrl::parse(
            "https://portal.example.com/team/invoices/9382?tenant=acme&token=secret#details",
        )
        .unwrap();
        assert_eq!(
            from_url(&url),
            "https://portal.example.com/team/invoices/*?tenant=*&token=*#*"
        );
    }

    #[test]
    fn keeps_a_short_folder_hint() {
        let url = ValidatedUrl::parse("https://example.com/dogs").unwrap();
        assert_eq!(from_url(&url), "https://example.com/dogs/*");
    }
}
