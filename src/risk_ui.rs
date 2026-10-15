//! 危険性と照会失敗を同じ起動前確認にまとめる。Enter・Esc・閉じるはキャンセル。
use crate::{app_title, risk::RiskReport, ui_support as common, url_input::ValidatedUrl};
use std::{
    mem::size_of,
    ptr::{null, null_mut},
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
use windows_sys::Win32::{
    Foundation::*, Graphics::Gdi::*, System::LibraryLoader::GetModuleHandleW,
    UI::WindowsAndMessaging::*,
};

const CONTINUE_ID: usize = 100;
const CANCEL_ID: usize = 2;

struct State {
    accepted: bool,
}

pub fn confirm(url: &ValidatedUrl, report: &RiskReport) -> bool {
    let _busy = crate::management::busy_guard();
    let mut state = State { accepted: false };
    let class = common::wide("HttpSelector.RiskWarning");
    let title = common::wide(&app_title::risk_warning());
    let mut wc: WNDCLASSEXW = unsafe { std::mem::zeroed() };
    wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
    wc.lpfnWndProc = Some(window_proc);
    wc.hInstance = unsafe { GetModuleHandleW(null()) };
    wc.hCursor = unsafe { LoadCursorW(null_mut(), IDC_ARROW) };
    wc.hbrBackground = (COLOR_WINDOW + 1) as HBRUSH;
    wc.lpszClassName = class.as_ptr();
    unsafe { RegisterClassExW(&wc) };
    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
    let bounds = common::initial_bounds(700, 510, style, WS_EX_APPWINDOW);
    let window = unsafe {
        CreateWindowExW(
            WS_EX_APPWINDOW,
            class.as_ptr(),
            title.as_ptr(),
            style,
            bounds.x,
            bounds.y,
            bounds.width,
            bounds.height,
            null_mut(),
            null_mut(),
            wc.hInstance,
            (&mut state as *mut State).cast(),
        )
    };
    if window.is_null() {
        return false;
    }
    let dpi = unsafe { windows_sys::Win32::UI::HiDpi::GetDpiForWindow(window) }.max(96);
    let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) as HFONT };
    let text = format!(
        "このリンクを開く前に確認してください。\r\n\r\n実際のアクセス先（ASCII）: {}\r\n元のドメイン表記: {}\r\n\r\n判定理由:\r\n{}\r\n\r\n元URL:\r\n{}\r\n\r\n判定は安全を保証するものではありません。",
        visible(&report.host),
        visible(&report.original_host),
        report
            .reasons
            .iter()
            .map(|s| format!("・{s}"))
            .collect::<Vec<_>>()
            .join("\r\n"),
        visible(url.original())
    );
    let edit = common::edit(window, 10, true, true, font);
    common::set_text(edit, &text);
    let cancel = common::control(
        window,
        "BUTTON",
        "キャンセル",
        CANCEL_ID as i32,
        WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
        font,
    );
    let proceed = common::control(
        window,
        "BUTTON",
        "それでも開く",
        CONTINUE_ID as i32,
        WS_TABSTOP | BS_PUSHBUTTON as u32,
        font,
    );
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    unsafe { GetClientRect(window, &mut rect) };
    let margin = common::scale(16, dpi);
    let height = common::scale(36, dpi);
    let width = common::scale(140, dpi);
    unsafe {
        MoveWindow(
            edit,
            margin,
            margin,
            rect.right - margin * 2,
            rect.bottom - margin * 3 - height,
            1,
        );
        MoveWindow(
            cancel,
            rect.right - margin * 2 - width * 2,
            rect.bottom - margin - height,
            width,
            height,
            1,
        );
        MoveWindow(
            proceed,
            rect.right - margin - width,
            rect.bottom - margin - height,
            width,
            height,
            1,
        );
        ShowWindow(window, SW_SHOW);
        UpdateWindow(window);
        SetForegroundWindow(window);
        SetFocus(cancel);
    }
    let mut message: MSG = unsafe { std::mem::zeroed() };
    loop {
        let status = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
        if status <= 0 {
            break;
        }
        // フォーカスが「それでも開く」以外なら、Enterもキャンセルとして扱う。
        if message.message == WM_KEYDOWN
            && (message.wParam == VK_ESCAPE_VALUE
                || (message.wParam == VK_RETURN_VALUE && unsafe { GetFocus() } != proceed))
        {
            unsafe { DestroyWindow(window) };
            continue;
        }
        if unsafe { IsDialogMessageW(window, &message) } == 0 {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
    state.accepted
}

const VK_ESCAPE_VALUE: usize = 0x1b;
const VK_RETURN_VALUE: usize = 0x0d;

fn visible(value: &str) -> String {
    value.chars().flat_map(|ch| {
        if ch.is_control() || matches!(ch, '\u{061c}' | '\u{200e}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') {
            format!("\\u{{{:04X}}}", ch as u32).chars().collect::<Vec<_>>()
        } else { vec![ch] }
    }).collect()
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = unsafe { &*(lparam as *const CREATESTRUCTW) };
        unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize) };
    }
    let pointer = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) as *mut State };
    match message {
        WM_COMMAND if !pointer.is_null() => {
            let id = wparam & 0xffff;
            if id == CONTINUE_ID || id == CANCEL_ID {
                unsafe {
                    (*pointer).accepted = id == CONTINUE_ID;
                    DestroyWindow(window);
                }
            }
            0
        }
        WM_CLOSE => {
            unsafe { DestroyWindow(window) };
            0
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            0
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn warning_does_not_allow_bidirectional_text_to_hide_the_host() {
        assert_eq!(visible("a\u{202e}b\n"), "a\\u{202E}b\\u{000A}");
    }
}
