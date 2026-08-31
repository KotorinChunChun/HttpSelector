use anyhow::{Context, ensure};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::{
    APPMODEL_ERROR_NO_PACKAGE, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS,
};
use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentPackageFamilyName;

/// Package Familyは更新で変わらず、開発版とStore版では異なる。
pub struct AppIdentity {
    package_family: Option<String>,
}

impl AppIdentity {
    pub fn current() -> anyhow::Result<&'static Self> {
        static IDENTITY: OnceLock<Result<AppIdentity, String>> = OnceLock::new();
        IDENTITY
            .get_or_init(|| Self::detect().map_err(|error| error.to_string()))
            .as_ref()
            .map_err(|error| anyhow::anyhow!("{error}"))
    }

    fn detect() -> anyhow::Result<Self> {
        let mut length = 0;
        let result = unsafe { GetCurrentPackageFamilyName(&mut length, std::ptr::null_mut()) };
        if result == APPMODEL_ERROR_NO_PACKAGE {
            return Ok(Self {
                package_family: None,
            });
        }
        ensure!(
            result == ERROR_INSUFFICIENT_BUFFER && length > 1,
            "パッケージIDを取得できません: {result}"
        );
        let mut buffer = vec![0_u16; length as usize];
        let result = unsafe { GetCurrentPackageFamilyName(&mut length, buffer.as_mut_ptr()) };
        ensure!(
            result == ERROR_SUCCESS,
            "パッケージIDを取得できません: {result}"
        );
        let family = String::from_utf16(&buffer[..length as usize - 1])
            .context("パッケージIDの形式が正しくありません")?;
        Self::packaged(family)
    }

    fn packaged(family: String) -> anyhow::Result<Self> {
        ensure!(
            !family.is_empty()
                && family
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b".-_".contains(&c))
                && family != "."
                && family != "..",
            "パッケージIDに不正な文字があります"
        );
        Ok(Self {
            package_family: Some(family),
        })
    }

    pub fn data_dir(&self, local_app_data: &Path) -> PathBuf {
        match &self.package_family {
            Some(family) => local_app_data
                .join("Packages")
                .join(family)
                .join("LocalState"),
            None => local_app_data.join("HttpSelector"),
        }
    }

    pub fn window_class(&self, base: &str) -> String {
        match &self.package_family {
            Some(family) => format!("{base}.{:x}", Sha256::digest(family.as_bytes())),
            None => base.to_owned(),
        }
    }

    pub fn mutex_name(&self) -> String {
        match &self.package_family {
            Some(_) => self.window_class("Local\\HttpSelector.SingleInstance"),
            // 既存の非MSIX版へのURL転送を維持する。
            None => "Local\\HttpSelector.SingleInstance.v0.0.1".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identities_isolate_data_and_ipc() {
        let loose = AppIdentity {
            package_family: None,
        };
        let beta = AppIdentity::packaged("HttpSelector.Development_abc123".into()).unwrap();
        let store = AppIdentity::packaged("Example.HttpSelector_def456".into()).unwrap();
        let root = Path::new("C:/Test/Local");
        assert_eq!(loose.data_dir(root), root.join("HttpSelector"));
        assert_eq!(
            beta.data_dir(root),
            root.join("Packages/HttpSelector.Development_abc123/LocalState")
        );
        for (first, second) in [(&loose, &beta), (&beta, &store), (&store, &loose)] {
            assert_ne!(first.data_dir(root), second.data_dir(root));
            assert_ne!(first.mutex_name(), second.mutex_name());
            assert_ne!(
                first.window_class("HttpSelector.MainWindow"),
                second.window_class("HttpSelector.MainWindow")
            );
        }
        assert_eq!(
            beta.mutex_name(),
            AppIdentity::packaged("HttpSelector.Development_abc123".into())
                .unwrap()
                .mutex_name()
        );
    }

    #[test]
    fn package_identity_cannot_escape_data_root() {
        for family in ["", "..", "../other", "C:\\other", "a\\b", "a/b"] {
            assert!(AppIdentity::packaged(family.into()).is_err());
        }
    }
}
