// CargoはSemVerの3桁に限定されるため、4桁の製品版を別に表示する。
pub const VERSION: &str = "0.0.3.6";

pub struct WindowTitle;

impl WindowTitle {
    pub fn create(screen: Option<&str>) -> String {
        let product = format!("HttpSelector - ver {VERSION}");
        match screen {
            Some(screen) => format!("{screen} - {product}"),
            None => product,
        }
    }
}

pub fn main() -> String {
    WindowTitle::create(None)
}

pub fn settings() -> String {
    WindowTitle::create(Some("設定"))
}

pub fn default_rules() -> String {
    WindowTitle::create(Some("既定ルール"))
}

pub fn settings_file_error() -> String {
    WindowTitle::create(Some("設定ファイルエラー"))
}

pub fn risk_warning() -> String {
    WindowTitle::create(Some("URLの安全性の確認"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_use_the_four_part_product_version() {
        assert_eq!(VERSION, env!("CARGO_PKG_VERSION").replace('+', "."));
        assert_eq!(main(), "HttpSelector - ver 0.0.3.6");
        assert_eq!(settings(), format!("設定 - {}", main()));
        assert_eq!(default_rules(), format!("既定ルール - {}", main()));
        assert_eq!(risk_warning(), format!("URLの安全性の確認 - {}", main()));
        assert_eq!(
            settings_file_error(),
            format!("設定ファイルエラー - {}", main())
        );
    }
}
