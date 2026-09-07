pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn main() -> String {
    format!("HttpSelector v{VERSION}")
}

pub fn settings() -> String {
    format!("{} - 設定", main())
}

pub fn default_rules() -> String {
    format!("{} - 既定ルール", main())
}

pub fn settings_file_error() -> String {
    format!("{} - 設定ファイルエラー", main())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_use_the_cargo_package_version() {
        assert_eq!(
            main(),
            format!("HttpSelector v{}", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(settings(), format!("{} - 設定", main()));
        assert_eq!(default_rules(), format!("{} - 既定ルール", main()));
        assert_eq!(
            settings_file_error(),
            format!("{} - 設定ファイルエラー", main())
        );
    }
}
