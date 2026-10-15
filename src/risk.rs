//! URLの検査結果と起動可否をまとめる。ネットワーク応答を安全保証とは扱わない。
use crate::{quad9, risk_local, url_input::ValidatedUrl};

#[derive(Clone, Debug)]
pub struct RiskReport {
    pub host: String,
    pub original_host: String,
    pub reasons: Vec<String>,
}

impl RiskReport {
    pub fn requires_confirmation(&self) -> bool {
        !self.reasons.is_empty()
    }
}

pub fn inspect(url: &ValidatedUrl) -> RiskReport {
    inspect_with(url, quad9::lookup)
}

pub fn inspect_with(
    url: &ValidatedUrl,
    lookup: impl FnOnce(&str) -> quad9::Reputation,
) -> RiskReport {
    let local = risk_local::assess(url);
    let mut reasons = local.findings;
    if let Some(domain) = local.external_domain {
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
    }
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
            let report = inspect_with(&url, |_| reputation);
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
