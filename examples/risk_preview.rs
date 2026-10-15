//! 実通信・設定保存・ブラウザー起動なしで警告の表示とキャンセルを確認する。
use http_selector::{
    risk::{RiskReport, guarded},
    risk_ui,
    url_input::ValidatedUrl,
};

fn main() -> anyhow::Result<()> {
    let url =
        ValidatedUrl::parse("https://ｇｏｏｇｌｅ.com.login.example/path?token=preview#section")?;
    let local = http_selector::risk_local::assess(&url);
    let report = RiskReport {
        host: local.host,
        original_host: local.original_host,
        reasons: local
            .findings
            .into_iter()
            .chain(["Quad9への照会結果を確認できませんでした。（表示テスト）".into()])
            .collect(),
    };
    let accepted = guarded(&report, |value| risk_ui::confirm(&url, value), || Ok(()))?;
    println!(
        "{}（プレビューのためブラウザー起動なし）",
        if accepted.is_some() {
            "続行を選択"
        } else {
            "キャンセル"
        }
    );
    Ok(())
}
