use crate::url_input::ValidatedUrl;
use percent_encoding::percent_decode_str;
use unicode_normalization::UnicodeNormalization;
use url::{Host, Url};

/// 元のURLを変更せずに行う判定。照会可能な場合も、外部へ渡すのはASCIIホストだけ。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalAssessment {
    pub host: String,
    pub original_host: String,
    pub findings: Vec<String>,
    pub external_domain: Option<String>,
}

struct Service {
    name: &'static str,
    token: &'static str,
    domains: &'static [&'static str],
}

// 正規ドメインの境界確認に使う一覧であり、安全性を保証する許可リストではない。
// この一覧に一致しても、外部ブロック判定は省略しない。
const SERVICES: &[Service] = &[
    Service {
        name: "Google",
        token: "google",
        domains: &[
            "google.com",
            "google.co.jp",
            "google.co.uk",
            "google.de",
            "google.fr",
            "google.ca",
            "google.com.au",
            "googleapis.com",
            "gstatic.com",
        ],
    },
    Service {
        name: "Microsoft",
        token: "microsoft",
        domains: &[
            "microsoft.com",
            "microsoftonline.com",
            "live.com",
            "outlook.com",
            "office.com",
            "office365.com",
            "azure.com",
            "windows.com",
            "bing.com",
            "msn.com",
            "aka.ms",
        ],
    },
    Service {
        name: "Apple",
        token: "apple",
        domains: &["apple.com", "icloud.com"],
    },
    Service {
        name: "Amazon",
        token: "amazon",
        domains: &[
            "amazon.co.jp",
            "amazon.com",
            "amazon.co.uk",
            "amazon.de",
            "amazon.fr",
            "amazon.ca",
            "amazon.it",
            "amazon.es",
            "amazon.com.au",
            "amazon.in",
            "amazon.com.br",
            "amazon.com.mx",
            "amazon.nl",
            "amazon.sg",
            "amazon.ae",
            "amazon.sa",
            "amazon.com.tr",
            "amazon.se",
            "amazon.pl",
            "amazon.eg",
            "amazon.com.be",
            "amazon.co.za",
            "amazon.ie",
        ],
    },
    Service {
        name: "PayPal",
        token: "paypal",
        domains: &["paypal.com", "paypalobjects.com"],
    },
    Service {
        name: "LINE",
        token: "line",
        domains: &["line.me", "linecorp.com", "line.biz", "lin.ee"],
    },
    Service {
        name: "Yahoo! JAPAN",
        token: "yahoo",
        domains: &["yahoo.co.jp", "yahoo.com", "yimg.jp"],
    },
    Service {
        name: "楽天",
        token: "rakuten",
        domains: &[
            "rakuten.co.jp",
            "rakuten.com",
            "rakuten-card.co.jp",
            "r10s.jp",
        ],
    },
];

const BAIT_WORDS: &[&str] = &[
    "account", "accounts", "auth", "billing", "confirm", "help", "id", "login", "my", "payment",
    "recovery", "secure", "security", "service", "signin", "support", "update", "verify",
];

pub fn assess(url: &ValidatedUrl) -> LocalAssessment {
    let parsed = Url::parse(url.original()).expect("ValidatedUrlには検証済みのURLが必要です");
    let host = parsed
        .host_str()
        .expect("ValidatedUrlにはホストが必要です")
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let original_host = original_host(url.original()).unwrap_or(&host).to_owned();
    let decoded_original = percent_decode_str(&original_host).decode_utf8_lossy();
    let mut findings = Vec::new();

    if decoded_original
        .chars()
        .any(|character| matches!(character, '\u{ff21}'..='\u{ff3a}' | '\u{ff41}'..='\u{ff5a}'))
    {
        findings.push("ホスト名に全角アルファベットが含まれています。通常の英字と見間違えるおそれがあります。".into());
    } else if decoded_original.chars().any(|character| {
        !character.is_ascii()
            && character
                .to_string()
                .nfkc()
                .any(|normalized| normalized.is_ascii_alphanumeric())
    }) {
        findings.push("ホスト名に通常の英数字に見える特殊なUnicode文字が含まれています。".into());
    }

    if !parsed.username().is_empty() || parsed.password().is_some() {
        findings.push(
            "URLに認証情報（@より前）が含まれています。その部分は実際の接続先ではありません。"
                .into(),
        );
    }

    let external_domain = match parsed.host() {
        Some(Host::Domain(_)) if public_dns_host(&host) => Some(host.clone()),
        _ => None,
    };

    if matches!(parsed.host(), Some(Host::Domain(_))) && !known_service_domain(&host) {
        let (unicode_host, decoded) = idna::domain_to_unicode(&host);
        if decoded.is_err() {
            findings.push("国際化ドメイン名の表記を正しく確認できませんでした。".into());
        } else {
            let registered = psl::domain_str(&host).unwrap_or(&host);
            let (registered_unicode, _) = idna::domain_to_unicode(registered);
            let registered_label = registered_unicode.split('.').next().unwrap_or_default();
            let labels: Vec<_> = unicode_host.split('.').collect();
            for service in SERVICES {
                if let Some(reason) = service_finding(service, &labels, registered_label) {
                    findings.push(reason);
                }
            }
        }
    }

    LocalAssessment {
        host,
        original_host,
        findings,
        external_domain,
    }
}

/// HTTP(S)のspecial URLに合わせて、逆スラッシュもauthorityの区切りとして扱う。
/// 認証情報・ポートは取り除くが、Unicodeとパーセント表記はそのまま保持する。
fn original_host(original: &str) -> Option<&str> {
    let (_, after_scheme) = original.split_once(':')?;
    let authority = after_scheme
        .trim_start_matches(['/', '\\'])
        .split(['/', '\\', '?', '#'])
        .next()?;
    let host_port = authority.rsplit('@').next()?;
    if host_port.starts_with('[') {
        return host_port.find(']').map(|end| &host_port[..=end]);
    }
    let host = host_port.split(':').next()?;
    (!host.is_empty()).then_some(host)
}

fn public_dns_host(host: &str) -> bool {
    const LOCAL_SUFFIXES: &[&str] = &[
        "localhost",
        "local",
        "localdomain",
        "internal",
        "intranet",
        "lan",
        "home",
        "corp",
        "home.arpa",
        "arpa",
        "onion",
        "test",
        "invalid",
        "example",
    ];
    if LOCAL_SUFFIXES
        .iter()
        .any(|suffix| at_domain_boundary(host, suffix))
        || host.len() > 253
        || host.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return false;
    }
    psl::suffix(host.as_bytes()).is_some_and(|suffix| suffix.is_known())
        && psl::domain_str(host).is_some()
}

fn at_domain_boundary(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

fn known_service_domain(host: &str) -> bool {
    SERVICES.iter().any(|service| {
        service
            .domains
            .iter()
            .any(|domain| at_domain_boundary(host, domain))
    })
}

fn service_finding(service: &Service, labels: &[&str], registered_label: &str) -> Option<String> {
    let brand_skeleton = skeleton(service.token);
    for token in labels.iter().flat_map(|label| label.split('-')) {
        if token == service.token || (service.token == "rakuten" && token == "楽天") {
            return Some(format!(
                "{}の名称を含みますが、確認済みの公式ドメインとは一致しません。偽サブドメインや偽装名の可能性があります。",
                service.name
            ));
        }
        if confusable_brand(token, &brand_skeleton)
            || leetspeak_matches(token, service.token)
            || decorated_brand(token, service.token)
        {
            return Some(format!(
                "{}の名称に似た文字や偽装用の語を含む、公式以外のホスト名です。",
                service.name
            ));
        }
    }
    // 短いLINEは一般語との衝突が多いため、編集距離で一律判定しない。
    // typoは登録可能ドメインのラベルに限定し、apply.example.com等の誤検知を抑える。
    let typo = registered_label.split('-').any(|token| {
        if service.token.len() < 5 {
            return token != service.token && without_repetitions(token) == service.token;
        }
        !matches!(token, "apply" | "ample" | "maple") && one_edit_or_swap(token, service.token)
    });
    typo.then(|| {
        format!(
            "登録ドメインが{}の名称の誤字（文字の置換・脱落・追加・入れ替え）に似ています。",
            service.name
        )
    })
}

fn skeleton(value: &str) -> String {
    let normalized: String = value.nfkc().collect();
    unicode_security::skeleton(&normalized)
        .collect::<String>()
        .to_lowercase()
}

fn confusable_brand(token: &str, brand_skeleton: &str) -> bool {
    if skeleton(token) == brand_skeleton {
        return true;
    }
    // Unicode 16のUTS39は小文字PALOCHKA（ӏ）をiへ対応付けるが、
    // フォントによってはlにも見える。ブランドとの完全一致になる場合だけ
    // lの代替形も照合し、一般のCyrillicドメインやASCIIのi/lを一律に疑わない。
    token.contains(['\u{04c0}', '\u{04cf}'])
        && skeleton(&token.replace(['\u{04c0}', '\u{04cf}'], "l")) == brand_skeleton
}

fn leetspeak_matches(token: &str, brand: &str) -> bool {
    ['l', 'i'].into_iter().any(|one| {
        let folded: String = token
            .chars()
            .map(|character| match character {
                '0' => 'o',
                '1' => one,
                '3' => 'e',
                '4' => 'a',
                '5' => 's',
                '7' => 't',
                other => other,
            })
            .collect();
        folded != token && folded == brand
    })
}

fn decorated_brand(token: &str, brand: &str) -> bool {
    BAIT_WORDS.iter().any(|bait| {
        token.strip_prefix(bait).is_some_and(|rest| rest == brand)
            || token.strip_suffix(bait).is_some_and(|rest| rest == brand)
    })
}

fn without_repetitions(value: &str) -> String {
    let mut previous = None;
    value
        .chars()
        .filter(|character| {
            let repeated = previous == Some(*character);
            previous = Some(*character);
            !repeated
        })
        .collect()
}

/// ブランド名は短いASCIIなので、距離行列を作らず1編集か隣接転置だけを調べる。
fn one_edit_or_swap(value: &str, target: &str) -> bool {
    if !value.is_ascii() || value == target || value.len().abs_diff(target.len()) > 1 {
        return false;
    }
    let value = value.as_bytes();
    let target = target.as_bytes();
    if value.len() == target.len() {
        let differences: Vec<_> = value
            .iter()
            .zip(target)
            .enumerate()
            .filter_map(|(index, (left, right))| (left != right).then_some(index))
            .collect();
        return match differences.as_slice() {
            [_] => true,
            [first, second] if *second == first + 1 => {
                value[*first] == target[*second] && value[*second] == target[*first]
            }
            _ => false,
        };
    }
    let (shorter, longer) = if value.len() < target.len() {
        (value, target)
    } else {
        (target, value)
    };
    let first_difference = shorter
        .iter()
        .zip(longer)
        .position(|(left, right)| left != right)
        .unwrap_or(shorter.len());
    shorter[first_difference..] == longer[first_difference + 1..]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assessment(value: &str) -> LocalAssessment {
        assess(&ValidatedUrl::parse(value).unwrap())
    }

    fn assert_warning(value: &str) {
        assert!(
            !assessment(value).findings.is_empty(),
            "警告が必要: {value}"
        );
    }

    #[test]
    fn official_domains_and_real_subdomains_do_not_trigger_brand_warnings() {
        for host in [
            "accounts.google.com",
            "google.co.jp",
            "login.microsoftonline.com",
            "www.microsoft.com",
            "appleid.apple.com",
            "www.amazon.co.jp",
            "www.amazon.com",
            "www.paypal.com",
            "access.line.me",
            "line.biz",
            "login.yahoo.co.jp",
            "login.account.rakuten.com",
            "www.rakuten.co.jp",
            "www.rakuten-card.co.jp",
            "GOOGLE.COM.",
        ] {
            let result = assessment(&format!("https://{host}/"));
            assert!(result.findings.is_empty(), "{host}: {:?}", result.findings);
            assert!(result.external_domain.is_some(), "{host}");
        }
    }

    #[test]
    fn fullwidth_host_is_detected_before_url_normalization() {
        let url = ValidatedUrl::parse("https://ｇｏｏｇｌｅ.com/private?secret=1").unwrap();
        let result = assess(&url);
        assert_eq!(result.host, "google.com");
        assert_eq!(result.original_host, "ｇｏｏｇｌｅ.com");
        assert!(result.findings.iter().any(|reason| reason.contains("全角")));
        assert_eq!(result.external_domain.as_deref(), Some("google.com"));
        assert_eq!(url.original(), "https://ｇｏｏｇｌｅ.com/private?secret=1");
        assert_warning("https://%EF%BD%87oogle.com/");
    }

    #[test]
    fn unicode_and_punycode_homographs_are_detected() {
        for value in [
            "https://gооgle.com/", // 2文字のCyrillic о。
            "https://аррӏе.com/",
            "https://аррӀе.com/",
            "https://xn--pple-43d.com/", // Cyrillic а + pple。
            "https://раypal.com/",
            "https://paypaӏ.com/",
            "https://googӏe.com/",
            "https://ӏine.com/",
            "https://arnazon.com/",
            "https://l1ne.com/",
        ] {
            assert_warning(value);
        }
    }

    #[test]
    fn typos_and_impersonation_tokens_cover_the_eight_services() {
        for host in [
            "goggle.com",
            "gooogle.com",
            "googel.com",
            "goole.com",
            "goog1e.com",
            "microsfot.com",
            "microsoft-login.com",
            "aple.com",
            "appel.com",
            "amaz0n.com",
            "paypa1.com",
            "line-login.com",
            "lline.com",
            "yahho.co.jp",
            "rakutne.co.jp",
            "楽天.com",
            "securegoogle.com",
            "accounts.google.com.evil.com",
            "google.com.evil.com",
            "apple.com.evil.com",
            "paypal.security.evil.com",
            "google.com.evil.invalid",
        ] {
            assert_warning(&format!("https://{host}/"));
        }
    }

    #[test]
    fn ordinary_domains_unicode_and_path_content_are_not_blanket_warnings() {
        for value in [
            "https://example.com/ｇｏｏｇｌｅ?next=paypal.com#Ａmazon",
            "https://example.com/%EF%BD%87oogle?service=google.com",
            "https://日本語.jp/",
            "https://bücher.de/",
            "https://пӏанета.рф/",
            "https://online.com/",
            "https://wine.com/",
            "https://pine.com/",
            "https://mine.com/",
            "https://apply.com/",
            "https://apply.example.com/",
            "https://google.com/search?q=ｇｏｏｇｌｅ",
        ] {
            let result = assessment(value);
            assert!(result.findings.is_empty(), "{value}: {:?}", result.findings);
        }
    }

    #[test]
    fn only_the_ascii_host_can_be_sent_externally() {
        let result =
            assessment("https://user:secret@Login.Example.COM:8443/private?token=secret#fragment");
        assert_eq!(result.external_domain.as_deref(), Some("login.example.com"));
        assert_eq!(result.original_host, "Login.Example.COM");
        assert!(
            result
                .findings
                .iter()
                .any(|reason| reason.contains("認証情報"))
        );
        assert!(
            result
                .findings
                .iter()
                .all(|reason| !reason.contains("secret"))
        );
        let result = assessment("https://例え.jp/");
        assert!(result.external_domain.as_deref().unwrap().is_ascii());
    }

    #[test]
    fn local_reserved_names_unknown_suffixes_and_all_ip_literals_are_not_sent() {
        for host in [
            "localhost",
            "printer",
            "nas.local",
            "db.internal",
            "router.home.arpa",
            "host.localhost",
            "mail.corp",
            "private.intranet",
            "service.invalid",
            "service.test",
            "unregistered.unrecognisedlocalsuffix",
            "127.0.0.1",
            "2130706433",
            "192.168.1.2",
            "10.0.0.1",
            "169.254.1.2",
            "100.64.0.1",
            "9.9.9.9",
            "[::1]",
            "[fd00::1]",
            "[fe80::1]",
            "[::ffff:192.168.1.1]",
        ] {
            let result = assessment(&format!("https://{host}/private?token=secret"));
            assert_eq!(result.external_domain, None, "{host}");
        }
        assert!(
            assessment("https://tenant.github.io/")
                .external_domain
                .is_some()
        );
        assert!(
            assessment("https://host.localhost.com/")
                .external_domain
                .is_some()
        );
    }

    #[test]
    fn authority_extraction_respects_at_sign_ports_and_backslash_boundaries() {
        let result = assessment("https://google.com@evil.com:8443/private");
        assert_eq!(result.host, "evil.com");
        assert_eq!(result.original_host, "evil.com");
        assert!(
            result
                .findings
                .iter()
                .any(|reason| reason.contains("認証情報"))
        );
        let result = assessment("https://example.com\\ｇｏｏｇｌｅ.com");
        assert_eq!(result.original_host, "example.com");
        assert!(result.findings.is_empty());
        assert_eq!(assessment("https://[::1]:8080/").original_host, "[::1]");
    }

    #[test]
    fn domain_boundary_and_one_edit_detection_do_not_accept_extra_suffixes() {
        assert!(known_service_domain("login.google.com"));
        assert!(!known_service_domain("notgoogle.com"));
        assert!(!known_service_domain("google.com.attacker.com"));
        assert!(!known_service_domain("google.com-attacker.com"));
        assert!(one_edit_or_swap("googel", "google"));
        assert!(one_edit_or_swap("goole", "google"));
        assert!(one_edit_or_swap("gooogle", "google"));
        assert!(!one_edit_or_swap("google", "google"));
        assert!(!one_edit_or_swap("example", "google"));
    }
}
