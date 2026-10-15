use anyhow::Context;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub database: PathBuf,
    pub signature: PathBuf,
    pub protected_key: PathBuf,
    pub database_backup: PathBuf,
    pub signature_backup: PathBuf,
    pub icon_dir: PathBuf,
}

impl AppPaths {
    pub fn for_current_user() -> anyhow::Result<Self> {
        #[cfg(debug_assertions)]
        if let Some(directory) = std::env::var_os("HTTPSELECTOR_TEST_DATA_DIR") {
            let directory = PathBuf::from(directory);
            anyhow::ensure!(
                directory.is_absolute(),
                "テスト用データ位置は絶対パスで指定してください"
            );
            return Ok(Self::from_data_dir(directory));
        }
        let local_app_data = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .context("LOCALAPPDATAが取得できません")?;
        Ok(Self::from_data_dir(
            crate::app_identity::AppIdentity::current()?.data_dir(&local_app_data),
        ))
    }

    pub fn from_data_dir(data_dir: impl Into<PathBuf>) -> Self {
        let data_dir = data_dir.into();
        Self {
            database: data_dir.join("HttpSelector.db"),
            signature: data_dir.join("HttpSelector.db.sig"),
            protected_key: data_dir.join("integrity.key"),
            database_backup: data_dir.join("HttpSelector.db.bak"),
            signature_backup: data_dir.join("HttpSelector.db.bak.sig"),
            icon_dir: data_dir.join("icons"),
            data_dir,
        }
    }

    pub fn ensure_data_dir(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.data_dir).with_context(|| {
            format!(
                "データフォルダーを作成できません: {}",
                self.data_dir.display()
            )
        })
    }

    pub fn is_inside_data_dir(&self, path: &Path) -> bool {
        path.starts_with(&self.data_dir)
    }
}
