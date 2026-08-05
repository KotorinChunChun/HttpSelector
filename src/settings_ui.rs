use crate::model::Profile;
use anyhow::ensure;
use std::mem::size_of;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{COLOR_WINDOW, DEFAULT_GUI_FONT, GetStockObject};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{BST_CHECKED, BST_UNCHECKED};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const WINDOW_CLASS: &str = "HttpSelector.SettingsWindow";
const ID_LIST: i32 = 100;
const ID_ALIAS: i32 = 101;
const ID_COMMENT: i32 = 102;
const ID_FAVORITE: i32 = 103;
const ID_ENABLED: i32 = 104;
const ID_UP: i32 = 105;
const ID_DOWN: i32 = 106;
const ID_SAVE: i32 = 107;
const ID_RESCAN: i32 = 108;
const ID_CANCEL: i32 = 109;
const ID_RECORD_URL_HISTORY: i32 = 110;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsAction {
    Saved,
    Rescan,
    Cancelled,
}

pub struct SettingsOutcome {
    pub action: SettingsAction,
    pub profiles: Vec<Profile>,
    pub pending_url: Option<crate::url_input::ValidatedUrl>,
    pub record_url_history: bool,
}

struct SettingsState {
    profiles: Vec<Profile>,
    list: HWND,
    alias: HWND,
    comment: HWND,
    favorite: HWND,
    enabled: HWND,
    record_url_history: HWND,
    record_url_history_enabled: bool,
    selected: Option<usize>,
    action: SettingsAction,
    pending_url: Option<crate::url_input::ValidatedUrl>,
}

impl SettingsState {
    fn new(mut profiles: Vec<Profile>, record_url_history_enabled: bool) -> Self {
        profiles.sort_by_key(|profile| (profile.sort_order, profile.id));
        Self {
            profiles,
            list: null_mut(),
            alias: null_mut(),
            comment: null_mut(),
            favorite: null_mut(),
            enabled: null_mut(),
            record_url_history: null_mut(),
            record_url_history_enabled,
            selected: None,
            action: SettingsAction::Cancelled,
            pending_url: None,
        }
    }
}

pub fn run_settings(
    profiles: Vec<Profile>,
    record_url_history_enabled: bool,
) -> anyhow::Result<SettingsOutcome> {
    register_window_class()?;
    let state_pointer = Box::into_raw(Box::new(SettingsState::new(
        profiles,
        record_url_history_enabled,
    )));
    let class = wide(WINDOW_CLASS);
    let title = wide("HttpSelector 設定");
    let width = 820;
    let height = 600;
    let x = ((unsafe { GetSystemMetrics(SM_CXSCREEN) } - width) / 2).max(0);
    let y = ((unsafe { GetSystemMetrics(SM_CYSCREEN) } - height) / 2).max(0);
    let window = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_CONTROLPARENT,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN,
            x,
            y,
            width,
            height,
            null_mut(),
            null_mut(),
            GetModuleHandleW(null()),
            state_pointer.cast(),
        )
    };
    if window.is_null() {
        unsafe { drop(Box::from_raw(state_pointer)) };
        anyhow::bail!("設定画面を作成できません: Win32 error {}", unsafe {
            GetLastError()
        });
    }
    unsafe {
        ShowWindow(window, SW_SHOW);
        SetForegroundWindow(window);
    }
    let mut message: MSG = unsafe { std::mem::zeroed() };
    loop {
        let status = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
        ensure!(status != -1, "設定画面のメッセージを取得できません");
        if status == 0 {
            break;
        }
        if unsafe { IsDialogMessageW(window, &message) } == 0 {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
    let state = unsafe { Box::from_raw(state_pointer) };
    Ok(SettingsOutcome {
        action: state.action,
        profiles: state.profiles,
        pending_url: state.pending_url,
        record_url_history: state.record_url_history_enabled,
    })
}

fn register_window_class() -> anyhow::Result<()> {
    let class = wide(WINDOW_CLASS);
    let mut window_class: WNDCLASSEXW = unsafe { std::mem::zeroed() };
    window_class.cbSize = size_of::<WNDCLASSEXW>() as u32;
    window_class.lpfnWndProc = Some(window_proc);
    window_class.hInstance = unsafe { GetModuleHandleW(null()) };
    window_class.hCursor = unsafe { LoadCursorW(null_mut(), IDC_ARROW) };
    window_class.hbrBackground = (COLOR_WINDOW + 1) as usize as _;
    window_class.lpszClassName = class.as_ptr();
    let atom = unsafe { RegisterClassExW(&window_class) };
    if atom == 0 {
        let error = unsafe { GetLastError() };
        ensure!(error == 1410, "設定画面クラスを登録できません: {error}");
    }
    Ok(())
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
    let pointer = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) as *mut SettingsState };
    match message {
        WM_CREATE => {
            if pointer.is_null() {
                return -1;
            }
            unsafe { create_controls(window, &mut *pointer) };
            0
        }
        WM_COMMAND => {
            if pointer.is_null() {
                return 0;
            }
            let state = unsafe { &mut *pointer };
            let id = low_word(wparam) as i32;
            let notification = high_word(wparam);
            if id == ID_LIST && notification == LBN_SELCHANGE as u16 {
                unsafe { change_selection(state) };
            } else if id == ID_UP && notification == BN_CLICKED as u16 {
                unsafe { move_selected(state, -1) };
            } else if id == ID_DOWN && notification == BN_CLICKED as u16 {
                unsafe { move_selected(state, 1) };
            } else if id == ID_SAVE && notification == BN_CLICKED as u16 {
                unsafe { save_current_editor(state) };
                state.record_url_history_enabled =
                    unsafe { SendMessageW(state.record_url_history, BM_GETCHECK, 0, 0) }
                        == BST_CHECKED as isize;
                normalize_sort_order(&mut state.profiles);
                state.action = SettingsAction::Saved;
                unsafe { DestroyWindow(window) };
            } else if id == ID_RESCAN && notification == BN_CLICKED as u16 {
                unsafe { save_current_editor(state) };
                state.action = SettingsAction::Rescan;
                unsafe { DestroyWindow(window) };
            } else if id == ID_CANCEL && notification == BN_CLICKED as u16 {
                state.action = SettingsAction::Cancelled;
                unsafe { DestroyWindow(window) };
            }
            0
        }
        WM_CLOSE => {
            if !pointer.is_null() {
                unsafe { (*pointer).action = SettingsAction::Cancelled };
            }
            unsafe { DestroyWindow(window) };
            0
        }
        WM_COPYDATA => {
            if pointer.is_null() {
                return 0;
            }
            let copy_data = unsafe { &*(lparam as *const COPYDATASTRUCT) };
            if copy_data.dwData != 1
                || copy_data.lpData.is_null()
                || copy_data.cbData < 2
                || copy_data.cbData % 2 != 0
                || copy_data.cbData as usize > (crate::url_input::MAX_URL_BYTES + 1) * 2
            {
                return 0;
            }
            let utf16 = unsafe {
                std::slice::from_raw_parts(
                    copy_data.lpData as *const u16,
                    copy_data.cbData as usize / 2,
                )
            };
            let end = utf16
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(utf16.len());
            let Ok(value) = String::from_utf16(&utf16[..end]) else {
                return 0;
            };
            let Ok(url) = crate::url_input::ValidatedUrl::parse(value) else {
                return 0;
            };
            unsafe { (*pointer).pending_url = Some(url) };
            unsafe {
                ShowWindow(window, SW_RESTORE);
                SetForegroundWindow(window);
            }
            1
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            0
        }
        WM_NCDESTROY => {
            unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, 0) };
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn create_controls(window: HWND, state: &mut SettingsState) {
    let font = GetStockObject(DEFAULT_GUI_FONT);
    create_static(window, "◫ プロファイル", 20, 18, 300, 24, font);
    state.list = create_control(
        window,
        "LISTBOX",
        "",
        ID_LIST,
        20,
        45,
        330,
        450,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER | WS_VSCROLL | LBS_NOTIFY as u32,
        font,
    );
    create_static(window, "✎ 表示名", 380, 55, 160, 24, font);
    state.alias = create_control(
        window,
        "EDIT",
        "",
        ID_ALIAS,
        380,
        80,
        400,
        30,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER | ES_AUTOHSCROLL as u32,
        font,
    );
    create_static(window, "☰ メモ", 380, 130, 160, 24, font);
    state.comment = create_control(
        window,
        "EDIT",
        "",
        ID_COMMENT,
        380,
        155,
        400,
        80,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER | ES_MULTILINE as u32,
        font,
    );
    state.favorite = create_control(
        window,
        "BUTTON",
        "お気に入り",
        ID_FAVORITE,
        380,
        260,
        170,
        32,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX as u32,
        font,
    );
    state.enabled = create_control(
        window,
        "BUTTON",
        "選択画面に表示",
        ID_ENABLED,
        580,
        260,
        180,
        32,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX as u32,
        font,
    );
    state.record_url_history = create_control(
        window,
        "BUTTON",
        "URLパターンを記録する（既定OFF）",
        ID_RECORD_URL_HISTORY,
        380,
        300,
        400,
        32,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX as u32,
        font,
    );
    SendMessageW(
        state.record_url_history,
        BM_SETCHECK,
        if state.record_url_history_enabled {
            BST_CHECKED as usize
        } else {
            BST_UNCHECKED as usize
        },
        0,
    );
    create_static(
        window,
        "URLは生の値ではなく、末尾・値・フラグメントを * に置き換えた\r\nパターンとして記録されます。例: /folder/*?tenant=*",
        380,
        345,
        400,
        55,
        font,
    );
    create_control(
        window,
        "BUTTON",
        "↑ 上へ",
        ID_UP,
        380,
        420,
        120,
        34,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
        font,
    );
    create_control(
        window,
        "BUTTON",
        "↓ 下へ",
        ID_DOWN,
        515,
        420,
        120,
        34,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
        font,
    );
    create_control(
        window,
        "BUTTON",
        "再スキャン",
        ID_RESCAN,
        650,
        420,
        130,
        34,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
        font,
    );
    create_control(
        window,
        "BUTTON",
        "保存",
        ID_SAVE,
        525,
        515,
        120,
        36,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
        font,
    );
    create_control(
        window,
        "BUTTON",
        "キャンセル",
        ID_CANCEL,
        660,
        515,
        120,
        36,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
        font,
    );
    refresh_list(state, Some(0));
}

unsafe fn change_selection(state: &mut SettingsState) {
    unsafe { save_current_editor(state) };
    let selected = unsafe { SendMessageW(state.list, LB_GETCURSEL, 0, 0) };
    if selected != LB_ERR as isize {
        state.selected = Some(selected as usize);
        unsafe { load_editor(state) };
    }
}

unsafe fn save_current_editor(state: &mut SettingsState) {
    let Some(index) = state.selected else {
        return;
    };
    if let Some(profile) = state.profiles.get_mut(index) {
        profile.alias_name = unsafe { window_text(state.alias) };
        profile.comment = unsafe { window_text(state.comment) };
        profile.favorite =
            unsafe { SendMessageW(state.favorite, BM_GETCHECK, 0, 0) } == BST_CHECKED as isize;
        profile.enabled =
            unsafe { SendMessageW(state.enabled, BM_GETCHECK, 0, 0) } == BST_CHECKED as isize;
    }
}

unsafe fn load_editor(state: &SettingsState) {
    let Some(profile) = state.selected.and_then(|index| state.profiles.get(index)) else {
        return;
    };
    let alias = wide(&profile.alias_name);
    let comment = wide(&profile.comment);
    unsafe {
        SetWindowTextW(state.alias, alias.as_ptr());
        SetWindowTextW(state.comment, comment.as_ptr());
        SendMessageW(
            state.favorite,
            BM_SETCHECK,
            if profile.favorite {
                BST_CHECKED as usize
            } else {
                BST_UNCHECKED as usize
            },
            0,
        );
        SendMessageW(
            state.enabled,
            BM_SETCHECK,
            if profile.enabled {
                BST_CHECKED as usize
            } else {
                BST_UNCHECKED as usize
            },
            0,
        );
    }
}

unsafe fn move_selected(state: &mut SettingsState, direction: isize) {
    unsafe { save_current_editor(state) };
    let Some(current) = state.selected else {
        return;
    };
    let target = current as isize + direction;
    if target < 0 || target >= state.profiles.len() as isize {
        return;
    }
    state.profiles.swap(current, target as usize);
    normalize_sort_order(&mut state.profiles);
    unsafe { refresh_list(state, Some(target as usize)) };
}

unsafe fn refresh_list(state: &mut SettingsState, selected: Option<usize>) {
    unsafe { SendMessageW(state.list, LB_RESETCONTENT, 0, 0) };
    for profile in &state.profiles {
        let availability = if profile.available {
            ""
        } else {
            " [未検出]"
        };
        let favorite = if profile.favorite { "★ " } else { "" };
        let text = wide(&format!(
            "{favorite}{} | {}{availability}",
            profile.browser_type.display_name(),
            profile.display_name()
        ));
        unsafe { SendMessageW(state.list, LB_ADDSTRING, 0, text.as_ptr() as isize) };
    }
    state.selected = selected.filter(|index| *index < state.profiles.len());
    if let Some(index) = state.selected {
        unsafe { SendMessageW(state.list, LB_SETCURSEL, index, 0) };
        unsafe { load_editor(state) };
    }
}

fn normalize_sort_order(profiles: &mut [Profile]) {
    for (index, profile) in profiles.iter_mut().enumerate() {
        profile.sort_order = index as i64;
    }
}

unsafe fn create_static(
    parent: HWND,
    text: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    font: *mut core::ffi::c_void,
) -> HWND {
    unsafe {
        create_control(
            parent,
            "STATIC",
            text,
            0,
            x,
            y,
            width,
            height,
            WS_CHILD | WS_VISIBLE,
            font,
        )
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn create_control(
    parent: HWND,
    class: &str,
    text: &str,
    id: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    style: u32,
    font: *mut core::ffi::c_void,
) -> HWND {
    let class = wide(class);
    let text = wide(text);
    let control = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            text.as_ptr(),
            style,
            x,
            y,
            width,
            height,
            parent,
            id as usize as _,
            GetModuleHandleW(null()),
            null_mut(),
        )
    };
    if !control.is_null() {
        unsafe { SendMessageW(control, WM_SETFONT, font as usize, 1) };
    }
    control
}

unsafe fn window_text(window: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(window) };
    let mut buffer = vec![0_u16; length as usize + 1];
    unsafe { GetWindowTextW(window, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..length as usize])
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

const fn low_word(value: usize) -> u16 {
    (value & 0xffff) as u16
}

const fn high_word(value: usize) -> u16 {
    ((value >> 16) & 0xffff) as u16
}
