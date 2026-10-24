use crate::url_input::ValidatedUrl;
use crate::win32::wide;
use anyhow::{Context, ensure};
use std::ptr::{null, null_mut};
use std::thread;
use std::time::Duration;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND,
};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, FindWindowW, GetWindowThreadProcessId, SMTO_ABORTIFHUNG, SW_RESTORE,
    SendMessageTimeoutW, SetForegroundWindow, ShowWindow, WM_COPYDATA,
};

const WINDOW_CLASSES: [&str; 4] = [
    crate::management::BROKER_CLASS,
    "HttpSelector.MainWindow",
    "HttpSelector.SettingsWindow",
    "HttpSelector.RuleWindow",
];

pub enum InstanceStatus {
    Primary(SingleInstanceGuard),
    Forwarded,
}

pub struct SingleInstanceGuard(HANDLE);

/// WM_COPYDATAが提供する有効な構造体とバッファを、URL受付の全画面で共通検証する。
///
/// # Safety
/// 非ゼロのlparamはWM_COPYDATAの有効期間内のCOPYDATASTRUCTを指す必要がある。
pub(crate) unsafe fn received_url(lparam: isize) -> Option<ValidatedUrl> {
    if lparam == 0 {
        return None;
    }
    let data = unsafe { &*(lparam as *const COPYDATASTRUCT) };
    if !matches!(data.dwData, 1 | 3)
        || data.lpData.is_null()
        || data.cbData < 2
        || data.cbData % 2 != 0
        || data.cbData as usize > (crate::url_input::MAX_URL_BYTES + 1) * 2
    {
        return None;
    }
    let encoded =
        unsafe { std::slice::from_raw_parts(data.lpData as *const u16, data.cbData as usize / 2) };
    let encoded = encoded.strip_suffix(&[0])?;
    Some(
        ValidatedUrl::parse(String::from_utf16(encoded).ok()?)
            .ok()?
            .with_forced_confirmation(data.dwData == 3),
    )
}

/// `received_url`が受け付ける形式でURLを送り、受信側の戻り値を返す。
/// 送信できなかった・時間切れの場合は`None`。
pub(crate) fn send_url(window: HWND, url: &ValidatedUrl, timeout_ms: u32) -> Option<usize> {
    let mut encoded = wide(url.original());
    let data = COPYDATASTRUCT {
        dwData: url.copy_data_kind(),
        cbData: (encoded.len() * size_of::<u16>()) as u32,
        lpData: encoded.as_mut_ptr().cast(),
    };
    let mut receiver_result = 0_usize;
    let sent = unsafe {
        SendMessageTimeoutW(
            window,
            WM_COPYDATA,
            0,
            (&data as *const COPYDATASTRUCT) as isize,
            SMTO_ABORTIFHUNG,
            timeout_ms,
            &mut receiver_result,
        )
    };
    (sent != 0).then_some(receiver_result)
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CloseHandle(self.0) };
        }
    }
}

pub fn acquire(url: Option<&ValidatedUrl>) -> anyhow::Result<InstanceStatus> {
    acquire_or_forward(
        &crate::app_identity::AppIdentity::current()?.mutex_name(),
        || forward_to_existing(url),
    )
}

fn acquire_or_forward(
    name: &str,
    forward: impl FnOnce() -> anyhow::Result<()>,
) -> anyhow::Result<InstanceStatus> {
    if let Some(guard) = try_acquire_primary(name)? {
        return Ok(InstanceStatus::Primary(guard));
    }
    if let Err(error) = forward() {
        // 既定ルールで先行プロセスが画面を作らず終了した場合は、後続URLを自分で処理する。
        if let Some(guard) = try_acquire_primary(name)? {
            return Ok(InstanceStatus::Primary(guard));
        }
        return Err(error);
    }
    Ok(InstanceStatus::Forwarded)
}

fn try_acquire_primary(name: &str) -> anyhow::Result<Option<SingleInstanceGuard>> {
    let name = wide(name);
    let handle = unsafe { CreateMutexW(null(), 0, name.as_ptr()) };
    if handle.is_null() {
        anyhow::bail!(
            "単一起動ミューテックスを作成できません: {}",
            unsafe { GetLastError() }
        );
    }
    let error = unsafe { GetLastError() };
    if error != ERROR_ALREADY_EXISTS {
        return Ok(Some(SingleInstanceGuard(handle)));
    }
    unsafe { CloseHandle(handle) };
    Ok(None)
}

fn forward_to_existing(url: Option<&ValidatedUrl>) -> anyhow::Result<()> {
    let identity = crate::app_identity::AppIdentity::current()?;
    let mut existing = null_mut();
    let mut is_broker = false;
    for _ in 0..40 {
        existing = WINDOW_CLASSES
            .iter()
            .find_map(|class_name| {
                let broker = *class_name == crate::management::BROKER_CLASS;
                let class_name = wide(&identity.window_class(class_name));
                let window = unsafe { FindWindowW(class_name.as_ptr(), null()) };
                if window.is_null() {
                    None
                } else {
                    is_broker = broker;
                    Some(window)
                }
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
    let mut process_id = 0;
    unsafe { GetWindowThreadProcessId(existing, &mut process_id) };
    let mut remote_session = 0;
    let mut own_session = 0;
    ensure!(
        unsafe { ProcessIdToSessionId(process_id, &mut remote_session) } != 0
            && unsafe { ProcessIdToSessionId(std::process::id(), &mut own_session) } != 0
            && remote_session == own_session,
        "別セッションのHttpSelectorへ転送できません"
    );
    unsafe { AllowSetForegroundWindow(process_id) };
    if let Some(url) = url {
        let receiver_result = send_url(existing, url, 2_000)
            .context("起動済みのHttpSelectorへURLを転送できません")?;
        ensure!(
            receiver_result != 0,
            "起動済みのHttpSelectorがURLを拒否しました"
        );
    } else if is_broker {
        let mut result = 0;
        ensure!(
            unsafe {
                SendMessageTimeoutW(
                    existing,
                    crate::management::WM_MANAGEMENT_SHOW,
                    0,
                    0,
                    SMTO_ABORTIFHUNG,
                    2_000,
                    &mut result,
                )
            } != 0,
            "起動済みのHttpSelectorへ表示を要求できません"
        );
    } else {
        unsafe {
            ShowWindow(existing, SW_RESTORE);
            SetForegroundWindow(existing);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_send_url_round_trip_preserves_shift_original_and_acceptance() {
        use windows_sys::Win32::UI::WindowsAndMessaging::DefWindowProcW;
        unsafe extern "system" fn receiver(
            window: HWND,
            message: u32,
            wparam: usize,
            lparam: isize,
        ) -> isize {
            if message == WM_COPYDATA {
                let state = unsafe {
                    crate::win32::window_state::<Option<ValidatedUrl>>(window, message, lparam)
                };
                unsafe {
                    *state = received_url(lparam);
                }
                return 1;
            }
            unsafe {
                crate::win32::window_state::<Option<ValidatedUrl>>(window, message, lparam);
                DefWindowProcW(window, message, wparam, lparam)
            }
        }
        let mut received = None;
        let window = crate::ui_support::HiddenTestWindow::new(
            Some(receiver),
            (&mut received as *mut Option<ValidatedUrl>).cast(),
        );
        let original = ValidatedUrl::parse("HTTPS://example.com/a?code=one%20time#private")
            .unwrap()
            .with_forced_confirmation(true);
        assert_eq!(send_url(window.0, &original, 250), Some(1));
        assert_eq!(received, Some(original.clone()));
        assert_eq!(send_url(null_mut(), &original, 10), None);
    }

    #[test]
    fn takes_over_if_the_primary_exits_before_url_forwarding() {
        let name = format!(
            "Local\\HttpSelector.Test.{}.{:?}",
            std::process::id(),
            std::thread::current().id()
        );
        let primary = try_acquire_primary(&name).unwrap().unwrap();
        assert!(try_acquire_primary(&name).unwrap().is_none());
        assert!(acquire_or_forward(&name, || anyhow::bail!("still running")).is_err());
        let result = acquire_or_forward(&name, || {
            drop(primary);
            anyhow::bail!("primary exited without a window")
        })
        .unwrap();
        assert!(matches!(result, InstanceStatus::Primary(_)));
    }

    #[test]
    fn ipc_rejects_invalid_payloads_and_preserves_the_original_url() {
        assert!(unsafe { received_url(0) }.is_none());
        let mut encoded = wide("https://example.com/a?x=%20");
        let mut data = COPYDATASTRUCT {
            dwData: 1,
            cbData: (encoded.len() * 2) as u32,
            lpData: encoded.as_mut_ptr().cast(),
        };
        let decode = |data: &COPYDATASTRUCT| unsafe { received_url(data as *const _ as isize) };
        assert_eq!(
            decode(&data).unwrap().original(),
            "https://example.com/a?x=%20"
        );
        data.dwData = 2;
        assert!(decode(&data).is_none());
        data.dwData = 3;
        let forced = decode(&data).unwrap();
        assert!(forced.force_confirmation());
        assert_eq!(forced.original(), "https://example.com/a?x=%20");
        assert_eq!(forced.copy_data_kind(), 3);
        assert!(
            !ValidatedUrl::parse(forced.original())
                .unwrap()
                .force_confirmation()
        );
        data.dwData = 1;
        data.cbData -= 1;
        assert!(decode(&data).is_none());
        data.cbData -= 1;
        assert!(decode(&data).is_none());
        data.cbData = ((crate::url_input::MAX_URL_BYTES + 2) * 2) as u32;
        assert!(decode(&data).is_none());
        data.cbData = 2;
        data.lpData = null_mut();
        assert!(decode(&data).is_none());
        for mut encoded in [
            wide("file:///C:/test"),
            wide("https://example.com/\0hidden"),
            vec![0xd800, 0],
        ] {
            let data = COPYDATASTRUCT {
                dwData: 1,
                cbData: (encoded.len() * 2) as u32,
                lpData: encoded.as_mut_ptr().cast(),
            };
            assert!(decode(&data).is_none());
        }
    }
}
