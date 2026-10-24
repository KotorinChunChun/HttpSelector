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
    let mut profiles: Vec<Profile> = if args.iter().any(|arg| arg == "--empty") {
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
    if args.iter().any(|arg| arg == "--guests") && profiles.len() >= 7 {
        for (index, browser_type) in [(5, BrowserType::Chrome), (6, BrowserType::Edge)] {
            let profile = &mut profiles[index];
            profile.browser_type = browser_type;
            profile.profile_id = http_selector::model::GUEST_PROFILE_ID.into();
            profile.detected_name = "ゲスト".into();
            profile.alias_name.clear();
            profile.comment.clear();
        }
    }
    if args
        .iter()
        .any(|arg| arg == "--association" || arg == "--teams" || arg == "--replace")
    {
        let profile = Profile {
            browser_type: BrowserType::Chrome,
            detected_name: "個人".into(),
            alias_name: String::new(),
            ..profiles[0].clone()
        };
        let url = if args.iter().any(|arg| arg == "--teams") {
            let mut url = url::Url::parse(http_selector::nested_url::TEAMS_WRAPPER_URL)?;
            url.query_pairs_mut().append_pair("url", "https://tenant-my.sharepoint.com/:f:/g/personal/person_example_com/sample-token?TeamsCID=11111111-2222-3333-4444-555555555555&view=preview")
                .append_pair("locale", "ja-jp").append_pair("pc", &"synthetic-token-not-real-".repeat(60)).append_pair("dest", "https://teams.microsoft.com/api/reputation");
            ValidatedUrl::parse(url.to_string())?
        } else {
            ValidatedUrl::parse(
                "https://apps.powerapps.com/play/e/default-11111111-2222-3333-4444-555555555555/a/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee?tenantId=11111111-2222-3333-4444-555555555555&hint=66666666-7777-8888-9999-000000000000&source=portal&screen=Requests&itemId=12345&locale=ja-JP&theme=light&mode=edit&hideNavBar=true&debug=false",
            )?
        };
        let existing = if args.iter().any(|arg| arg == "--replace") {
            let parts = http_selector::association::UrlParts::new(&url);
            let mut selection = parts.defaults();
            selection.queries[0] = true;
            Some(http_selector::model::DefaultRule {
                id: 1,
                profile_id: 1,
                pattern: http_selector::association::StoredRule::new(
                    parts.condition(&selection)?,
                    false,
                )
                .encode()?,
                enabled: true,
                sort_order: 0,
                created_at: 0,
                updated_at: 0,
            })
        } else {
            None
        };
        let result = http_selector::association_ui::run_with_existing(
            &url,
            &profile,
            existing.as_ref(),
            |rule| {
                println!("保存プレビュー: {}（保存先なし）", rule.condition.summary());
                Ok(())
            },
        )?;
        println!("連携プレビュー: {:?}（ブラウザ起動なし）", result.action);
    } else if args.iter().any(|arg| arg == "--settings") {
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
            SelectorResult::RuleList { .. } => {
                http_selector::default_rule_ui::run_rules(profiles, vec![], None)?;
            }
            result => println!("選択プレビュー: {result:?}（ブラウザ起動なし）"),
        }
    }
    Ok(())
}
