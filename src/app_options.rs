//! 単体・管理・隔離試験の起動引数を、実処理と分離して解釈する。
use crate::url_input::ValidatedUrl;
use anyhow::Context;

pub struct LaunchOptions {
    pub startup: bool,
    pub origin: String,
    pub url: Option<ValidatedUrl>,
}

pub fn test_scan_disabled() -> bool {
    cfg!(debug_assertions)
        && std::env::var_os("HTTPSELECTOR_TEST_DATA_DIR").is_some()
        && std::env::var_os("HTTPSELECTOR_TEST_NO_SCAN").is_some()
}

pub fn parse_arguments(arguments: &[String]) -> anyhow::Result<LaunchOptions> {
    let mut options = LaunchOptions {
        startup: false,
        origin: "standalone".into(),
        url: None,
    };
    let mut values = arguments.iter();
    while let Some(value) = values.next() {
        match value.as_str() {
            "--startup" => options.startup = true,
            "--test-isolated" => {
                anyhow::ensure!(
                    test_scan_disabled()
                        && std::env::var_os("HTTPSELECTOR_TEST_DATA_DIR")
                            .is_some_and(|value| std::path::Path::new(&value).is_absolute()),
                    "隔離試験はdebug版と絶対パスの専用保存先が必要です"
                );
            }
            "--launch-origin" => {
                let origin = values.next().context("起動経路が指定されていません")?;
                anyhow::ensure!(
                    matches!(
                        origin.as_str(),
                        "standalone" | "host_manual" | "direct_task" | "via_kotorintoys"
                    ),
                    "起動経路が正しくありません"
                );
                options.origin = origin.clone();
            }
            _ => {
                anyhow::ensure!(options.url.is_none(), "URLは1件ずつ指定してください");
                options.url = Some(ValidatedUrl::parse(value.clone())?);
            }
        }
    }
    Ok(options)
}
