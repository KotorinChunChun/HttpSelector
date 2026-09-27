// CargoはSemVerの3桁に限定されるため、4桁の製品版を別に表示する。
pub const VERSION: &str = "0.0.3.1";

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
    fn titles_use_the_four_part_product_version() {
        assert_eq!(main(), "HttpSelector v0.0.3.1");
        assert_eq!(settings(), format!("{} - 設定", main()));
        assert_eq!(default_rules(), format!("{} - 既定ルール", main()));
        assert_eq!(
            settings_file_error(),
            format!("{} - 設定ファイルエラー", main())
        );
    }
}
