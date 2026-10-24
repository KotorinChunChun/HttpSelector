//! URLの検査結果と起動可否をまとめる。ネットワーク応答を安全保証とは扱わない。
use crate::{quad9, risk_local, url_input::ValidatedUrl};

#[derive(Clone, Debug)]
pub struct RiskReport {
    pub host: String,
    pub original_host: String,
    pub reasons: Vec<String>,
    pub approvable_domains: Vec<String>,
}

impl RiskReport {
    pub fn requires_confirmation(&self) -> bool {
        !self.reasons.is_empty()
    }
}

pub fn inspect(url: &ValidatedUrl) -> RiskReport {
    inspect_approved(url, &[])
}

pub fn inspect_approved(url: &ValidatedUrl, approved: &[String]) -> RiskReport {
    inspect_approved_with_quad9(url, approved, false)
}

pub fn inspect_approved_with_quad9(
    url: &ValidatedUrl,
    approved: &[String],
    enabled: bool,
) -> RiskReport {
    inspect_with_policy(url, approved, enabled, &mut quad9::lookup)
}

pub fn inspect_with(
    url: &ValidatedUrl,
    mut lookup: impl FnMut(&str) -> quad9::Reputation,
) -> RiskReport {
    inspect_with_approved(url, &[], &mut lookup)
}

fn single(
    url: &ValidatedUrl,
    approved: &[String],
    external_enabled: bool,
    lookup: &mut impl FnMut(&str) -> quad9::Reputation,
) -> RiskReport {
    let local = risk_local::assess(url);
    let approved = approved.iter().any(|h| h == &local.host);
    let mut reasons = local
        .findings
        .into_iter()
        .filter(|reason| !approved || !local.name_findings.contains(reason))
        .collect::<Vec<_>>();
    let approvable_domains = if !approved && !local.name_findings.is_empty() {
        vec![local.host.clone()]
    } else {
        vec![]
    };
    if external_enabled && let Some(domain) = local.external_domain {
        match lookup(&domain) {
            quad9::Reputation::Clear => {}
            quad9::Reputation::Blocked => {
                reasons.push("Quad9がこのドメインを脅威としてブロックしています。".into());
            }
            quad9::Reputation::Unavailable(_) => {
                reasons.push("Quad9に接続できないか、照会結果を確認できませんでした。".into());
            }
        }
    }
    RiskReport {
        host: local.host,
        original_host: local.original_host,
        reasons,
        approvable_domains,
    }
}

pub fn inspect_with_approved(
    url: &ValidatedUrl,
    approved: &[String],
    lookup: &mut impl FnMut(&str) -> quad9::Reputation,
) -> RiskReport {
    inspect_with_policy(url, approved, true, lookup)
}

pub fn inspect_with_policy(
    url: &ValidatedUrl,
    approved: &[String],
    external_enabled: bool,
    lookup: &mut impl FnMut(&str) -> quad9::Reputation,
) -> RiskReport {
    let mut report = single(url, approved, external_enabled, lookup);
    if crate::nested_url::is_teams_link(url) && !crate::nested_url::is_teams_wrapper(url) {
        report
            .reasons
            .push("Teamsの中継URLが通常のHTTPS形式ではないため、リンク先を確認できません。".into());
    }
    let mut current = url.clone();
    let mut depth = 0;
    while crate::nested_url::is_teams_wrapper(&current) {
        if depth >= crate::nested_url::MAX_DEPTH {
            report
                .reasons
                .push("リンク先URLの入れ子が深すぎるため確認できません。".into());
            break;
        }
        if let Some(target) = crate::nested_url::teams_target(&current) {
            let inner = single(&target, approved, external_enabled, lookup);
            report.reasons.extend(
                inner
                    .reasons
                    .into_iter()
                    .map(|r| format!("リンク先 {}: {r}", inner.host)),
            );
            report.approvable_domains.extend(inner.approvable_domains);
            current = target;
            depth += 1;
        } else {
            report.reasons.push(
                "Teamsのリンク先urlが欠落・重複しているか、HTTP(S) URLとして確認できません。"
                    .into(),
            );
            break;
        }
    }
    report
}

pub fn normalize_approved_domains(domains: &[String]) -> anyhow::Result<Vec<String>> {
    anyhow::ensure!(domains.len() <= 256, "承認ドメインが多すぎます");
    let mut result = vec![];
    for value in domains {
        let host = value.trim().trim_end_matches('.').to_ascii_lowercase();
        anyhow::ensure!(
            host.is_ascii()
                && host.contains('.')
                && host.len() <= 253
                && host.split('.').all(|p| !p.is_empty()
                    && !p.starts_with('-')
                    && !p.ends_with('-')
                    && p.len() <= 63
                    && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')),
            "承認ドメインはASCIIの完全なホスト名で指定してください"
        );
        if !result.contains(&host) {
            result.push(host);
        }
    }
    Ok(result)
}

/// 承認前にはブラウザー起動やルール・履歴の保存を実行しない。
pub fn guarded<T>(
    report: &RiskReport,
    confirm: impl FnOnce(&RiskReport) -> bool,
    action: impl FnOnce() -> anyhow::Result<T>,
) -> anyhow::Result<Option<T>> {
    if report.requires_confirmation() && !confirm(report) {
        return Ok(None);
    }
    action().map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn external_lookup_requires_opt_in_for_outer_and_inner_urls() {
        let safe = ValidatedUrl::parse("https://example.com/a?secret=private").unwrap();
        let mut wrapper = url::Url::parse("https://teams.public.onecdn.static.microsoft/evergreen-assets/safelinks/2/atp-safelinks.html").unwrap();
        wrapper
            .query_pairs_mut()
            .append_pair("url", "https://microsfot.com/a?token=private");
        let wrapped = ValidatedUrl::parse(wrapper.to_string()).unwrap();
        let mut lookup = |_: &str| -> quad9::Reputation {
            panic!("OFF時は通信関数を呼んではいけません")
        };
        assert!(!inspect_with_policy(&safe, &[], false, &mut lookup).requires_confirmation());
        assert!(inspect_with_policy(&wrapped, &[], false, &mut lookup).requires_confirmation());
        assert!(!inspect(&safe).requires_confirmation());
        let mut hosts = vec![];
        let report = inspect_with_policy(&wrapped, &[], true, &mut |host| {
            hosts.push(host.to_owned());
            quad9::Reputation::Unavailable("offline".into())
        });
        assert_eq!(
            hosts,
            ["teams.public.onecdn.static.microsoft", "microsfot.com"]
        );
        assert!(report.reasons.iter().any(|reason| reason.contains("Quad9")));
        assert!(!hosts.iter().any(|host| host.contains("private")));
    }

    #[test]
    fn approval_is_exact_and_only_suppresses_name_findings() {
        let url = ValidatedUrl::parse("https://microsoft-login.example/path").unwrap();
        let clear = &mut |_: &str| quad9::Reputation::Clear;
        assert!(inspect_with_approved(&url, &[], clear).requires_confirmation());
        assert!(
            !inspect_with_approved(&url, &["microsoft-login.example".into()], clear)
                .requires_confirmation()
        );
        let credential =
            ValidatedUrl::parse("https://user:secret@microsoft-login.example/").unwrap();
        assert!(
            inspect_with_approved(&credential, &["microsoft-login.example".into()], clear)
                .reasons
                .iter()
                .any(|s| s.contains("認証情報"))
        );
        let lookalike = ValidatedUrl::parse("https://microsoft-login.example.evil.com/").unwrap();
        assert!(
            inspect_with_approved(&lookalike, &["microsoft-login.example".into()], clear)
                .requires_confirmation()
        );
        let fullwidth = ValidatedUrl::parse("https://ｍicrosoft-login.example/").unwrap();
        assert!(
            inspect_with_approved(&fullwidth, &["microsoft-login.example".into()], clear)
                .requires_confirmation()
        );
        for value in [
            "*.example",
            "https://example.com",
            "user@example.com",
            "example.com:443",
            "example.com/path",
        ] {
            assert!(normalize_approved_domains(&[value.into()]).is_err());
        }
    }

    #[test]
    fn official_wrapper_does_not_trust_its_target_or_skip_block_checks() {
        let mut url = url::Url::parse("https://teams.public.onecdn.static.microsoft/evergreen-assets/safelinks/2/atp-safelinks.html").unwrap();
        url.query_pairs_mut()
            .append_pair("url", "https://microsfot.com/a?private=not-sent");
        let url = ValidatedUrl::parse(url.to_string()).unwrap();
        let mut hosts = vec![];
        let report = inspect_with_approved(&url, &[], &mut |host| {
            hosts.push(host.to_owned());
            quad9::Reputation::Clear
        });
        assert_eq!(
            hosts,
            ["teams.public.onecdn.static.microsoft", "microsfot.com"]
        );
        assert!(
            report
                .reasons
                .iter()
                .any(|r| r.contains("リンク先 microsfot.com"))
        );
        assert!(
            inspect_with_approved(&url, &["microsfot.com".into()], &mut |_| {
                quad9::Reputation::Blocked
            })
            .requires_confirmation()
        );
        assert!(
            inspect_with_approved(&url, &["microsfot.com".into()], &mut |_| {
                quad9::Reputation::Unavailable("offline".into())
            })
            .requires_confirmation()
        );
        assert!(
            !inspect_with_approved(&url, &["microsfot.com".into()], &mut |_| {
                quad9::Reputation::Clear
            })
            .requires_confirmation()
        );
    }

    #[test]
    fn sends_only_host_and_preserves_the_original_url() {
        let input = "https://login.example.com/path?token=secret#fragment";
        let url = ValidatedUrl::parse(input).unwrap();
        let report = inspect_with(&url, |domain| {
            assert_eq!(domain, "login.example.com");
            quad9::Reputation::Clear
        });
        assert!(!report.requires_confirmation());
        assert_eq!(url.original(), input);
    }

    #[test]
    fn cancellation_prevents_all_launch_side_effects() {
        let url = ValidatedUrl::parse("https://example.com/").unwrap();
        for reputation in [
            quad9::Reputation::Blocked,
            quad9::Reputation::Unavailable("offline".into()),
        ] {
            let report = inspect_with(&url, |_| reputation.clone());
            let launched = Cell::new(false);
            let result = guarded(
                &report,
                |_| false,
                || {
                    launched.set(true);
                    Ok(())
                },
            )
            .unwrap();
            assert!(result.is_none());
            assert!(!launched.get());
            assert!(guarded(&report, |_| true, || Ok(42)).unwrap().is_some());
        }
    }

    #[test]
    fn private_hosts_are_not_disclosed() {
        for input in [
            "http://localhost/a?secret=1",
            "http://127.0.0.1/",
            "http://192.168.1.1/",
            "http://internal.local/",
        ] {
            let url = ValidatedUrl::parse(input).unwrap();
            inspect_with(&url, |_| panic!("非公開ホストを外部送信してはいけません"));
        }
    }
}
