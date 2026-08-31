use crate::url_input::ValidatedUrl;
use anyhow::ensure;
use std::ptr::{null, null_mut};
use std::thread;
use std::time::Duration;
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, SMTO_ABORTIFHUNG, SW_RESTORE, SendMessageTimeoutW, SetForegroundWindow,
    ShowWindow, WM_COPYDATA,
};

const WINDOW_CLASSES: [&str; 3] = [
    "HttpSelector.MainWindow",
    "HttpSelector.SettingsWindow",
    "HttpSelector.RuleWindow",
];

pub enum InstanceStatus {
    Primary(SingleInstanceGuard),
    Forwarded,
}

pub struct SingleInstanceGuard(HANDLE);

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CloseHandle(self.0) };
        }
    }
}

pub fn acquire(url: Option<&ValidatedUrl>) -> anyhow::Result<InstanceStatus> {
    let name = wide(&crate::app_identity::AppIdentity::current()?.mutex_name());
    let handle = unsafe { CreateMutexW(null(), 0, name.as_ptr()) };
    if handle.is_null() {
        anyhow::bail!(
            "単一起動ミューテックスを作成できません: {}",
            unsafe { GetLastError() }
        );
    }
    let error = unsafe { GetLastError() };
    if error != ERROR_ALREADY_EXISTS {
        return Ok(InstanceStatus::Primary(SingleInstanceGuard(handle)));
    }
    unsafe { CloseHandle(handle) };
    forward_to_existing(url)?;
    Ok(InstanceStatus::Forwarded)
}

fn forward_to_existing(url: Option<&ValidatedUrl>) -> anyhow::Result<()> {
    let identity = crate::app_identity::AppIdentity::current()?;
    let mut existing = null_mut();
    for _ in 0..40 {
        existing = WINDOW_CLASSES
            .iter()
            .find_map(|class_name| {
                let class_name = wide(&identity.window_class(class_name));
                let window = unsafe { FindWindowW(class_name.as_ptr(), null()) };
                (!window.is_null()).then_some(window)
            })
            .unwrap_or(null_mut());
        if !existing.is_null() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    ensure!(
        !existing.is_null(),
        "起動済みのHttpSelector画面が見つかりません"
    );
    if let Some(url) = url {
        let mut encoded = wide(url.original());
        let copy_data = COPYDATASTRUCT {
            dwData: 1,
            cbData: (encoded.len() * size_of::<u16>()) as u32,
            lpData: encoded.as_mut_ptr().cast(),
        };
        let mut receiver_result = 0_usize;
        let sent = unsafe {
            SendMessageTimeoutW(
                existing,
                WM_COPYDATA,
                0,
                (&copy_data as *const COPYDATASTRUCT) as isize,
                SMTO_ABORTIFHUNG,
                2_000,
                &mut receiver_result,
            )
        };
        ensure!(sent != 0, "起動済みのHttpSelectorへURLを転送できません");
        ensure!(
            receiver_result != 0,
            "起動済みのHttpSelectorがURLを拒否しました"
        );
    }
    unsafe {
        ShowWindow(existing, SW_RESTORE);
        SetForegroundWindow(existing);
    }
    Ok(())
}

fn size_of<T>() -> usize {
    std::mem::size_of::<T>()
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
