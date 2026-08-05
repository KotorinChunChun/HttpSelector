use crate::app_paths::AppPaths;
use crate::model::{BrowserType, DetectedProfile, Profile};
use crate::url_input::ValidatedUrl;
use anyhow::{Context, bail};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, SelectObject,
};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DI_NORMAL, DestroyIcon, DrawIconEx, HICON, PrivateExtractIconsW,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserCommand {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
}

pub fn build_command(executable: PathBuf, profile: &Profile, url: &ValidatedUrl) -> BrowserCommand {
    let mut arguments = match profile.browser_type {
        BrowserType::Edge | BrowserType::Chrome => {
            vec![format!("--profile-directory={}", profile.profile_id)]
        }
        BrowserType::Firefox => vec!["-P".to_owned(), profile.profile_id.clone()],
    };
    arguments.push(url.original().to_owned());
    BrowserCommand {
        executable,
        arguments,
    }
}

pub fn launch(profile: &Profile, url: &ValidatedUrl) -> anyhow::Result<()> {
    let executable = resolve_executable(profile.browser_type)?;
    let command = build_command(executable, profile, url);
    Command::new(&command.executable)
        .args(&command.arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| {
            format!(
                "{}を起動できません: {}",
                profile.browser_type,
                command.executable.display()
            )
        })?;
    Ok(())
}

pub fn cache_detected_icons(
    paths: &AppPaths,
    profiles: &mut [DetectedProfile],
) -> anyhow::Result<()> {
    std::fs::create_dir_all(&paths.icon_dir)?;
    for browser_type in BrowserType::ALL {
        if !profiles
            .iter()
            .any(|profile| profile.browser_type == browser_type)
        {
            continue;
        }
        let icon_path = cache_browser_icon(paths, browser_type);
        for profile in profiles
            .iter_mut()
            .filter(|profile| profile.browser_type == browser_type)
        {
            profile.icon_path = icon_path.clone();
        }
    }
    Ok(())
}

fn cache_browser_icon(paths: &AppPaths, browser_type: BrowserType) -> Option<String> {
    let destination = paths.icon_dir.join(format!("{}.bmp", browser_type as i64));
    if !destination.is_file() {
        let executable = resolve_executable(browser_type).ok()?;
        extract_icon_as_bmp(&executable, &destination).ok()?;
    }
    destination.to_str().map(str::to_owned)
}

fn extract_icon_as_bmp(executable: &Path, destination: &Path) -> anyhow::Result<()> {
    const ICON_SIZE: i32 = 32;
    let executable = wide(&executable.to_string_lossy());
    let mut icon: HICON = std::ptr::null_mut();
    let extracted = unsafe {
        PrivateExtractIconsW(
            executable.as_ptr(),
            0,
            ICON_SIZE,
            ICON_SIZE,
            &mut icon,
            std::ptr::null_mut(),
            1,
            0,
        )
    };
    if extracted == 0 || icon.is_null() {
        anyhow::bail!("ブラウザアイコンを抽出できません");
    }

    let bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: ICON_SIZE,
            biHeight: -ICON_SIZE,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            biSizeImage: (ICON_SIZE * ICON_SIZE * 4) as u32,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    let dc = unsafe { CreateCompatibleDC(std::ptr::null_mut()) };
    if dc.is_null() {
        unsafe { DestroyIcon(icon) };
        anyhow::bail!("アイコン描画用DCを作成できません");
    }
    let bitmap = unsafe {
        CreateDIBSection(
            dc,
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        )
    };
    if bitmap.is_null() || bits.is_null() {
        unsafe {
            DeleteDC(dc);
            DestroyIcon(icon);
        }
        anyhow::bail!("アイコンキャッシュ用ビットマップを作成できません");
    }
    let old = unsafe { SelectObject(dc, bitmap.cast()) };
    let drawn = unsafe {
        DrawIconEx(
            dc,
            0,
            0,
            icon,
            ICON_SIZE,
            ICON_SIZE,
            0,
            std::ptr::null_mut(),
            DI_NORMAL,
        )
    };
    let pixels = unsafe {
        std::slice::from_raw_parts(bits as *const u8, (ICON_SIZE * ICON_SIZE * 4) as usize).to_vec()
    };
    unsafe {
        SelectObject(dc, old);
        DeleteObject(bitmap.cast());
        DeleteDC(dc);
        DestroyIcon(icon);
    }
    if drawn == 0 {
        anyhow::bail!("ブラウザアイコンを描画できません");
    }

    let mut bmp = Vec::with_capacity(14 + 40 + pixels.len());
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&((14 + 40 + pixels.len()) as u32).to_le_bytes());
    bmp.extend_from_slice(&0_u16.to_le_bytes());
    bmp.extend_from_slice(&0_u16.to_le_bytes());
    bmp.extend_from_slice(&54_u32.to_le_bytes());
    bmp.extend_from_slice(&40_u32.to_le_bytes());
    bmp.extend_from_slice(&ICON_SIZE.to_le_bytes());
    bmp.extend_from_slice(&(-ICON_SIZE).to_le_bytes());
    bmp.extend_from_slice(&1_u16.to_le_bytes());
    bmp.extend_from_slice(&32_u16.to_le_bytes());
    bmp.extend_from_slice(&0_u32.to_le_bytes());
    bmp.extend_from_slice(&(pixels.len() as u32).to_le_bytes());
    bmp.extend_from_slice(&0_i32.to_le_bytes());
    bmp.extend_from_slice(&0_i32.to_le_bytes());
    bmp.extend_from_slice(&0_u32.to_le_bytes());
    bmp.extend_from_slice(&0_u32.to_le_bytes());
    bmp.extend_from_slice(&pixels);
    std::fs::write(destination, bmp).with_context(|| {
        format!(
            "アイコンキャッシュを保存できません: {}",
            destination.display()
        )
    })?;
    Ok(())
}

pub fn resolve_executable(browser_type: BrowserType) -> anyhow::Result<PathBuf> {
    let mut candidates = registry_executables(browser_type);
    candidates.extend(candidate_executables(browser_type));
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| anyhow::anyhow!("{}の実行ファイルが見つかりません", browser_type))
}

fn registry_executables(browser_type: BrowserType) -> Vec<PathBuf> {
    let executable_name = match browser_type {
        BrowserType::Edge => "msedge.exe",
        BrowserType::Chrome => "chrome.exe",
        BrowserType::Firefox => "firefox.exe",
    };
    let key = format!("Software\\Microsoft\\Windows\\CurrentVersion\\App Paths\\{executable_name}");
    [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
        .into_iter()
        .filter_map(|root| read_default_registry_string(root, &key))
        .map(PathBuf::from)
        .collect()
}

fn read_default_registry_string(root: HKEY, subkey: &str) -> Option<String> {
    let subkey = wide(subkey);
    let mut byte_count = 0_u32;
    let first = unsafe {
        RegGetValueW(
            root,
            subkey.as_ptr(),
            std::ptr::null(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut byte_count,
        )
    };
    if first != 0 || !(2..=64 * 1024).contains(&byte_count) {
        return None;
    }
    let mut buffer = vec![0_u16; byte_count as usize / 2];
    let second = unsafe {
        RegGetValueW(
            root,
            subkey.as_ptr(),
            std::ptr::null(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut byte_count,
        )
    };
    if second != 0 {
        return None;
    }
    let end = buffer
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(buffer.len());
    String::from_utf16(&buffer[..end]).ok()
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn candidate_executables(browser_type: BrowserType) -> Vec<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let program_files = std::env::var_os("ProgramFiles").map(PathBuf::from);
    let program_files_x86 = std::env::var_os("ProgramFiles(x86)").map(PathBuf::from);

    let relative_paths: &[&str] = match browser_type {
        BrowserType::Edge => &["Microsoft/Edge/Application/msedge.exe"],
        BrowserType::Chrome => &["Google/Chrome/Application/chrome.exe"],
        BrowserType::Firefox => &["Mozilla Firefox/firefox.exe"],
    };

    let roots: Vec<PathBuf> = match browser_type {
        BrowserType::Edge | BrowserType::Chrome => [local, program_files, program_files_x86]
            .into_iter()
            .flatten()
            .collect(),
        BrowserType::Firefox => [program_files, program_files_x86]
            .into_iter()
            .flatten()
            .collect(),
    };

    roots
        .into_iter()
        .flat_map(|root| {
            relative_paths
                .iter()
                .map(move |path| join_portable(&root, path))
        })
        .collect()
}

fn join_portable(root: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

pub fn ensure_profile_is_launchable(profile: &Profile) -> anyhow::Result<()> {
    if profile.profile_id.is_empty()
        || profile
            .profile_id
            .chars()
            .any(|character| character.is_control())
    {
        bail!("プロファイル識別子が不正です");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(browser_type: BrowserType, profile_id: &str) -> Profile {
        Profile {
            id: 1,
            browser_type,
            profile_id: profile_id.to_owned(),
            detected_name: "Work".to_owned(),
            alias_name: String::new(),
            comment: String::new(),
            sort_order: 0,
            favorite: false,
            enabled: true,
            available: true,
            icon_path: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn chromium_arguments_are_separate_from_url() {
        let profile = profile(BrowserType::Edge, "Profile 2");
        let url = ValidatedUrl::parse("https://example.com/?x=1&y=2").unwrap();
        let command = build_command(PathBuf::from("msedge.exe"), &profile, &url);
        assert_eq!(
            command.arguments,
            [
                "--profile-directory=Profile 2",
                "https://example.com/?x=1&y=2"
            ]
        );
    }

    #[test]
    fn firefox_uses_named_profile() {
        let profile = profile(BrowserType::Firefox, "work");
        let url = ValidatedUrl::parse("https://example.com").unwrap();
        let command = build_command(PathBuf::from("firefox.exe"), &profile, &url);
        assert_eq!(command.arguments, ["-P", "work", "https://example.com"]);
    }

    #[test]
    fn extracts_a_windows_icon_to_bmp_cache() {
        let windows = std::env::var_os("WINDIR").unwrap();
        let executable = PathBuf::from(windows).join("System32").join("notepad.exe");
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("notepad.bmp");
        extract_icon_as_bmp(&executable, &output).unwrap();
        assert_eq!(&std::fs::read(output).unwrap()[..2], b"BM");
    }
}
