use crate::app_paths::AppPaths;
use anyhow::{Context, ensure};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom, CRYPT_INTEGER_BLOB,
    CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};

const KEY_LENGTH: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntegrityStatus {
    Missing,
    Valid,
    Invalid,
}

pub struct IntegrityManager<'a> {
    paths: &'a AppPaths,
}

impl<'a> IntegrityManager<'a> {
    pub fn new(paths: &'a AppPaths) -> Self {
        Self { paths }
    }

    pub fn verify_current(&self) -> anyhow::Result<IntegrityStatus> {
        self.verify_pair(&self.paths.database, &self.paths.signature)
    }

    pub fn sign_current(&self) -> anyhow::Result<()> {
        let key = self.load_or_create_key()?;
        let signature = calculate_hmac(&self.paths.database, &key)?;
        atomic_write(&self.paths.signature, &signature)
    }

    pub fn backup_current(&self) -> anyhow::Result<()> {
        ensure!(
            self.verify_current()? == IntegrityStatus::Valid,
            "署名が有効でないDBはバックアップできません"
        );
        std::fs::copy(&self.paths.database, &self.paths.database_backup)
            .context("DBバックアップを作成できません")?;
        std::fs::copy(&self.paths.signature, &self.paths.signature_backup)
            .context("署名バックアップを作成できません")?;
        Ok(())
    }

    pub fn verify_backup(&self) -> anyhow::Result<IntegrityStatus> {
        self.verify_pair(&self.paths.database_backup, &self.paths.signature_backup)
    }

    pub fn restore_backup(&self) -> anyhow::Result<()> {
        ensure!(
            self.verify_backup()? == IntegrityStatus::Valid,
            "有効なバックアップがありません"
        );
        std::fs::copy(&self.paths.database_backup, &self.paths.database)
            .context("DBバックアップを復元できません")?;
        std::fs::copy(&self.paths.signature_backup, &self.paths.signature)
            .context("署名バックアップを復元できません")?;
        Ok(())
    }

    pub fn quarantine_current(&self) -> anyhow::Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if self.paths.database.exists() {
            let destination = self
                .paths
                .data_dir
                .join(format!("HttpSelector.db.corrupt-{timestamp}"));
            std::fs::rename(&self.paths.database, destination).context("破損DBを退避できません")?;
        }
        if self.paths.signature.exists() {
            let destination = self
                .paths
                .data_dir
                .join(format!("HttpSelector.db.sig.corrupt-{timestamp}"));
            std::fs::rename(&self.paths.signature, destination)
                .context("破損署名を退避できません")?;
        }
        if self.paths.protected_key.exists() {
            let destination = self
                .paths
                .data_dir
                .join(format!("integrity.key.corrupt-{timestamp}"));
            std::fs::rename(&self.paths.protected_key, destination)
                .context("破損した保護鍵を退避できません")?;
        }
        Ok(())
    }

    fn verify_pair(&self, database: &Path, signature: &Path) -> anyhow::Result<IntegrityStatus> {
        if !database.exists() {
            return Ok(IntegrityStatus::Missing);
        }
        if !signature.is_file() || !self.paths.protected_key.is_file() {
            return Ok(IntegrityStatus::Invalid);
        }
        let key = match self.load_key() {
            Ok(key) => key,
            Err(_) => return Ok(IntegrityStatus::Invalid),
        };
        let expected = std::fs::read(signature)
            .with_context(|| format!("署名を読み取れません: {}", signature.display()))?;
        let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("HMAC accepts any key length");
        update_mac_from_file(database, &mut mac)?;
        Ok(if mac.verify_slice(&expected).is_ok() {
            IntegrityStatus::Valid
        } else {
            IntegrityStatus::Invalid
        })
    }

    fn load_or_create_key(&self) -> anyhow::Result<Vec<u8>> {
        if self.paths.protected_key.is_file() {
            return self.load_key();
        }
        let mut key = vec![0_u8; KEY_LENGTH];
        let status = unsafe {
            BCryptGenRandom(
                std::ptr::null_mut(),
                key.as_mut_ptr(),
                key.len() as u32,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        };
        ensure!(
            status == 0,
            "暗号学的乱数を生成できません: NTSTATUS {status}"
        );
        let protected = protect_with_dpapi(&key)?;
        atomic_write(&self.paths.protected_key, &protected)?;
        Ok(key)
    }

    fn load_key(&self) -> anyhow::Result<Vec<u8>> {
        let protected = std::fs::read(&self.paths.protected_key).with_context(|| {
            format!(
                "保護鍵を読み取れません: {}",
                self.paths.protected_key.display()
            )
        })?;
        let key = unprotect_with_dpapi(&protected)?;
        ensure!(key.len() == KEY_LENGTH, "HMAC鍵の長さが不正です");
        Ok(key)
    }
}

fn calculate_hmac(path: &Path, key: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    update_mac_from_file(path, &mut mac)?;
    Ok(mac.finalize().into_bytes().to_vec())
}

fn update_mac_from_file(path: &Path, mac: &mut Hmac<Sha256>) -> anyhow::Result<()> {
    let mut file =
        File::open(path).with_context(|| format!("署名対象を開けません: {}", path.display()))?;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        mac.update(&buffer[..read]);
    }
    Ok(())
}

fn protect_with_dpapi(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let succeeded = unsafe {
        CryptProtectData(
            &input,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    ensure!(succeeded != 0, "DPAPIで鍵を保護できません");
    copy_and_free_blob(output)
}

fn unprotect_with_dpapi(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let succeeded = unsafe {
        CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    ensure!(succeeded != 0, "DPAPIで鍵を復号できません");
    copy_and_free_blob(output)
}

fn copy_and_free_blob(blob: CRYPT_INTEGER_BLOB) -> anyhow::Result<Vec<u8>> {
    ensure!(!blob.pbData.is_null(), "DPAPIから空の結果が返されました");
    let copied = unsafe { std::slice::from_raw_parts(blob.pbData, blob.cbData as usize) }.to_vec();
    unsafe {
        LocalFree(blob.pbData.cast());
    }
    Ok(copied)
}

fn atomic_write(path: &Path, contents: &[u8]) -> anyhow::Result<()> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .context("保存先ファイル名が不正です")?;
    let temporary = path.with_file_name(format!(".{file_name}.tmp"));
    {
        let mut file = File::create(&temporary)
            .with_context(|| format!("一時ファイルを作成できません: {}", temporary.display()))?;
        use std::io::Write;
        file.write_all(contents)?;
        file.sync_all()?;
    }
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    std::fs::rename(&temporary, path)
        .with_context(|| format!("ファイルを確定できません: {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_tampering_and_restores_backup() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path());
        paths.ensure_data_dir().unwrap();
        std::fs::write(&paths.database, b"first").unwrap();
        let manager = IntegrityManager::new(&paths);
        manager.sign_current().unwrap();
        assert_eq!(manager.verify_current().unwrap(), IntegrityStatus::Valid);
        manager.backup_current().unwrap();

        std::fs::write(&paths.database, b"tampered").unwrap();
        assert_eq!(manager.verify_current().unwrap(), IntegrityStatus::Invalid);
        manager.restore_backup().unwrap();
        assert_eq!(manager.verify_current().unwrap(), IntegrityStatus::Valid);
        assert_eq!(std::fs::read(&paths.database).unwrap(), b"first");
    }
}
