//! 画面・IPC・ブラウザー起動で共通に使うWin32の小さな変換処理。
use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, GWLP_USERDATA, GetWindowLongPtrW, SetWindowLongPtrW, WM_NCCREATE,
};

/// Win32 APIへ渡すNUL終端のUTF-16文字列を作る。
pub fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// WM_NCCREATEで渡された状態をウィンドウへ結び付け、保存済みの状態を返す。
/// 結び付け前と、WM_NCDESTROYで解除した後はnullを返す。
///
/// # Safety
/// `lparam`は`message`に対応する値であること。WM_NCCREATEの`lpCreateParams`は
/// `T`型の状態か、nullを指すこと。
pub unsafe fn window_state<T>(window: HWND, message: u32, lparam: LPARAM) -> *mut T {
    if message == WM_NCCREATE {
        let create = unsafe { &*(lparam as *const CREATESTRUCTW) };
        unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize) };
    }
    unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) as *mut T }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_strings_are_nul_terminated_utf16() {
        assert_eq!(wide(""), [0]);
        assert_eq!(wide("aあ"), ['a' as u16, 'あ' as u16, 0]);
    }
}
