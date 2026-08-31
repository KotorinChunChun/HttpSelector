//! 実データやブラウザ起動に触れず、長文・最大件数・空状態を確認するUIプレビュー。
use http_selector::model::{BrowserType, Profile};
use http_selector::settings_ui::run_settings;
use http_selector::storage::{
    DEFAULT_ESCAPE_HOLD_DURATION_TENTHS, DEFAULT_OPEN_PROFILE_ON_DOUBLE_CLICK,
};
use http_selector::ui::SelectorResult;
#[cfg(not(debug_assertions))]
use http_selector::ui::run_selector;
#[cfg(debug_assertions)]
use http_selector::ui::run_selector_preview as run_selector;
use http_selector::url_input::ValidatedUrl;

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let profiles = if args.iter().any(|arg| arg == "--empty") {
        vec![]
    } else {
        (0..25)
            .map(|index| Profile {
                id: index + 1,
                browser_type: BrowserType::ALL[index as usize % 3],
                profile_id: format!("Preview {index}"),
                detected_name: format!("プロファイル {}", index + 1),
                alias_name: if index == 0 {
                    "長い表示名の確認：開発・検証用プロファイル & サポート".into()
                } else {
                    format!(
                        "{} {}",
                        ["仕事用", "個人用", "検証用"][index as usize % 3],
                        index + 1
                    )
                },
                comment: "複数行のメモも領域内で省略します。\r\n次の行に続く説明です。".into(),
                sort_order: index,
                favorite: index < 5,
                enabled: true,
                available: true,
                icon_path: None,
                created_at: 0,
                updated_at: 0,
            })
            .collect()
    };
    if args.iter().any(|arg| arg == "--settings") {
        let result = run_settings(
            profiles,
            false,
            DEFAULT_ESCAPE_HOLD_DURATION_TENTHS,
            DEFAULT_OPEN_PROFILE_ON_DOUBLE_CLICK,
        )?;
        println!("設定プレビュー: {:?}（保存先なし）", result.action);
    } else {
        let url = if args.iter().any(|arg| arg == "--no-url") {
            None
        } else {
            Some(ValidatedUrl::parse(
                "https://example.com/projects/%E9%96%8B%E7%99%BA/details?message=long%20URL%20preview&environment=testing#section",
            )?)
        };
        match run_selector(
            url,
            profiles.clone(),
            DEFAULT_ESCAPE_HOLD_DURATION_TENTHS,
            DEFAULT_OPEN_PROFILE_ON_DOUBLE_CLICK,
        )? {
            SelectorResult::Settings { .. } => {
                run_settings(
                    profiles,
                    false,
                    DEFAULT_ESCAPE_HOLD_DURATION_TENTHS,
                    DEFAULT_OPEN_PROFILE_ON_DOUBLE_CLICK,
                )?;
            }
            result => println!("選択プレビュー: {result:?}（ブラウザ起動なし）"),
        }
    }
    Ok(())
}
