use crate::app_paths::AppPaths;
use crate::model::{BrowserType, DetectedProfile, Profile};
use crate::url_input::ValidatedUrl;
use anyhow::{Context, bail};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, SelectObject,
};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, DI_NORMAL, DestroyIcon, DrawIconEx, EnumWindows, GW_OWNER,
    GetForegroundWindow, GetWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
    HICON, IsIconic, IsWindowVisible, PrivateExtractIconsW, SW_RESTORE, SetForegroundWindow,
    ShowWindowAsync,
};

const ACTIVATION_GRACE: Duration = Duration::from_millis(500);
const ACTIVATION_TIMEOUT: Duration = Duration::from_millis(2_500);
const ACTIVATION_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Clone, Debug, Eq, PartialEq)]
struct BrowserWindow {
    handle: usize,
    process_id: u32,
    title: String,
}

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
    let before = browser_windows(&command.executable);
    grant_foreground_permission(&before);
    let child = Command::new(&command.executable)
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
    unsafe {
        AllowSetForegroundWindow(child.id());
    }
    activate_browser_window(&command.executable, &before);
    Ok(())
}

fn activate_browser_window(executable: &Path, before: &[BrowserWindow]) {
    let started = Instant::now();
    loop {
        let current = browser_windows(executable);
        grant_foreground_permission(&current);
        if foreground_is_in(&current) {
            return;
        }
        let changed_window = changed_browser_window(before, &current);
        if (changed_window.is_some() || started.elapsed() >= ACTIVATION_GRACE)
            && let Some(handle) = choose_browser_window(before, &current)
        {
            let window = handle as HWND;
            unsafe {
                if IsIconic(window) != 0 {
                    ShowWindowAsync(window, SW_RESTORE);
                }
                SetForegroundWindow(window);
            }
            if foreground_is_in(&current) {
                return;
            }
        }
        if started.elapsed() >= ACTIVATION_TIMEOUT {
            return;
        }
        thread::sleep(ACTIVATION_POLL_INTERVAL);
    }
}

fn choose_browser_window(before: &[BrowserWindow], current: &[BrowserWindow]) -> Option<usize> {
    changed_browser_window(before, current).or_else(|| current.first().map(|window| window.handle))
}

fn changed_browser_window(before: &[BrowserWindow], current: &[BrowserWindow]) -> Option<usize> {
    current
        .iter()
        .find(|window| !before.iter().any(|old| old.handle == window.handle))
        .or_else(|| {
            current.iter().find(|window| {
                before
                    .iter()
                    .find(|old| old.handle == window.handle)
                    .is_some_and(|old| old.title != window.title)
            })
        })
        .or_else(|| {
            current
                .iter()
                .enumerate()
                .find(|(current_index, window)| {
                    before
                        .iter()
                        .position(|old| old.handle == window.handle)
                        .is_some_and(|before_index| *current_index < before_index)
                })
                .map(|(_, window)| window)
        })
        .map(|window| window.handle)
}

fn foreground_is_in(windows: &[BrowserWindow]) -> bool {
    let foreground = unsafe { GetForegroundWindow() } as usize;
    foreground != 0 && windows.iter().any(|window| window.handle == foreground)
}

fn grant_foreground_permission(windows: &[BrowserWindow]) {
    for (index, window) in windows.iter().enumerate() {
        if windows[..index]
            .iter()
            .any(|other| other.process_id == window.process_id)
        {
            continue;
        }
        unsafe {
            AllowSetForegroundWindow(window.process_id);
        }
    }
}

fn browser_windows(executable: &Path) -> Vec<BrowserWindow> {
    let expected = normalized_executable(executable);
    visible_root_windows()
        .into_iter()
        .filter_map(|window| {
            let mut process_id = 0;
            unsafe {
                GetWindowThreadProcessId(window, &mut process_id);
            }
            let process_executable = process_executable(process_id)?;
            (normalized_executable(&process_executable) == expected).then(|| BrowserWindow {
                handle: window as usize,
                process_id,
                title: window_title(window),
            })
        })
        .collect()
}

fn normalized_executable(path: &Path) -> String {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    path.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('/', "\\")
        .to_lowercase()
}

fn process_executable(process_id: u32) -> Option<PathBuf> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if process.is_null() {
        return None;
    }
    let mut buffer = vec![0_u16; 32_768];
    let mut length = buffer.len() as u32;
    let result =
        unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length) };
    unsafe {
        CloseHandle(process);
    }
    (result != 0 && length > 0)
        .then(|| PathBuf::from(String::from_utf16_lossy(&buffer[..length as usize])))
}

fn window_title(window: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(window) };
    if length <= 0 {
        return String::new();
    }
    let mut buffer = vec![0_u16; length as usize + 1];
    let copied = unsafe { GetWindowTextW(window, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..copied.max(0) as usize])
}

fn visible_root_windows() -> Vec<HWND> {
    unsafe extern "system" fn collect(window: HWND, parameter: LPARAM) -> i32 {
        if unsafe { IsWindowVisible(window) } != 0
            && unsafe { GetWindow(window, GW_OWNER) }.is_null()
        {
            let windows = unsafe { &mut *(parameter as *mut Vec<HWND>) };
            windows.push(window);
        }
        1
    }

    let mut windows = Vec::new();
    unsafe {
        EnumWindows(Some(collect), (&mut windows as *mut Vec<HWND>) as LPARAM);
    }
    windows
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
    fn a_new_browser_window_is_preferred() {
        let before = vec![BrowserWindow {
            handle: 10,
            process_id: 1,
            title: "以前のページ".into(),
        }];
        let current = vec![
            BrowserWindow {
                handle: 20,
                process_id: 2,
                title: "新しいページ".into(),
            },
            before[0].clone(),
        ];
        assert_eq!(choose_browser_window(&before, &current), Some(20));
    }

    #[test]
    fn a_window_with_a_changed_title_is_preferred_for_an_existing_browser() {
        let before = vec![
            BrowserWindow {
                handle: 10,
                process_id: 1,
                title: "個人用".into(),
            },
            BrowserWindow {
                handle: 20,
                process_id: 1,
                title: "作業用".into(),
            },
        ];
        let current = vec![
            BrowserWindow {
                handle: 10,
                process_id: 1,
                title: "個人用".into(),
            },
            BrowserWindow {
                handle: 20,
                process_id: 1,
                title: "開いたページ".into(),
            },
        ];
        assert_eq!(choose_browser_window(&before, &current), Some(20));
    }

    #[test]
    fn a_browser_window_moved_forward_in_z_order_is_preferred() {
        let before = vec![
            BrowserWindow {
                handle: 10,
                process_id: 1,
                title: "個人用".into(),
            },
            BrowserWindow {
                handle: 20,
                process_id: 1,
                title: "作業用".into(),
            },
        ];
        let current = vec![before[1].clone(), before[0].clone()];
        assert_eq!(choose_browser_window(&before, &current), Some(20));
    }

    #[test]
    fn the_front_browser_window_is_the_fallback() {
        let current = vec![BrowserWindow {
            handle: 30,
            process_id: 3,
            title: "変更なし".into(),
        }];
        assert_eq!(choose_browser_window(&current, &current), Some(30));
        assert_eq!(choose_browser_window(&current, &[]), None);
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
