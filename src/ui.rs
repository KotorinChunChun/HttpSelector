use crate::model::Profile;
use crate::url_input::ValidatedUrl;
use anyhow::ensure;
use std::collections::HashMap;
use std::mem::size_of;
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    COLOR_WINDOW, CreateSolidBrush, DEFAULT_GUI_FONT, DeleteObject, FillRect, GetMonitorInfoW,
    GetStockObject, HBITMAP, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, SetBkColor,
    SetTextColor, UpdateWindow,
};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::WC_BUTTONW;
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetFocus, SetFocus, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_RETURN, VK_RIGHT, VK_UP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const WINDOW_CLASS: &str = "HttpSelector.MainWindow";
const WINDOW_WIDTH: i32 = 1160;
const WINDOW_HEIGHT: i32 = 840;
const MAX_TILES: usize = 25;
const HOLD_DURATION: Duration = Duration::from_secs(3);
const TIMER_HOLD: usize = 1;

const ID_SEARCH: i32 = 100;
const ID_SETTINGS: i32 = 101;
const ID_CLOSE: i32 = 102;
const ID_PROFILE_BASE: i32 = 1000;
const ID_PROFILE_ICON_BASE: i32 = 2000;
const APP_BACKGROUND: u32 = 0x00FAF7F5;
const APP_TEXT: u32 = 0x00413A32;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SelectorResult {
    Profile {
        profile_id: i64,
        url: Option<ValidatedUrl>,
    },
    Settings {
        url: Option<ValidatedUrl>,
    },
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntegrityRecoveryAction {
    Restore,
    Initialize,
    Exit,
}

struct UiState {
    url: Option<ValidatedUrl>,
    profiles: Vec<Profile>,
    filtered_indices: Vec<Option<usize>>,
    profile_buttons: Vec<HWND>,
    profile_images: Vec<HWND>,
    loaded_bitmaps: HashMap<String, HBITMAP>,
    background_brush: *mut core::ffi::c_void,
    search: HWND,
    original_edit: HWND,
    preview_edit: HWND,
    close_button: HWND,
    close_started: Option<Instant>,
    escape_started: Option<Instant>,
    result: SelectorResult,
}

impl UiState {
    fn new(url: Option<ValidatedUrl>, profiles: Vec<Profile>) -> Self {
        let filtered_indices = filtered_slots(&profiles, "");
        Self {
            url,
            profiles,
            filtered_indices,
            profile_buttons: Vec::new(),
            profile_images: Vec::new(),
            loaded_bitmaps: HashMap::new(),
            background_brush: null_mut(),
            search: null_mut(),
            original_edit: null_mut(),
            preview_edit: null_mut(),
            close_button: null_mut(),
            close_started: None,
            escape_started: None,
            result: SelectorResult::Cancelled,
        }
    }
}

pub fn run_selector(
    url: Option<ValidatedUrl>,
    profiles: Vec<Profile>,
) -> anyhow::Result<SelectorResult> {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    register_window_class()?;

    let state = Box::new(UiState::new(url, profiles));
    let state_pointer = Box::into_raw(state);
    let (x, y) = centered_on_cursor_monitor(WINDOW_WIDTH, WINDOW_HEIGHT);
    let title = wide("HttpSelector");
    let class = wide(WINDOW_CLASS);
    let instance = unsafe { GetModuleHandleW(null()) };
    let window = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_CONTROLPARENT,
            class.as_ptr(),
            title.as_ptr(),
            WS_POPUP | WS_CLIPCHILDREN,
            x,
            y,
            WINDOW_WIDTH,
            WINDOW_HEIGHT,
            null_mut(),
            null_mut(),
            instance,
            state_pointer.cast(),
        )
    };
    if window.is_null() {
        unsafe {
            drop(Box::from_raw(state_pointer));
        }
        anyhow::bail!("選択画面を作成できません: Win32 error {}", unsafe {
            GetLastError()
        });
    }

    unsafe {
        ShowWindow(window, SW_SHOW);
        UpdateWindow(window);
        SetForegroundWindow(window);
    }

    let mut message: MSG = unsafe { std::mem::zeroed() };
    loop {
        let status = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
        ensure!(status != -1, "Windowsメッセージを取得できません");
        if status == 0 {
            break;
        }
        if handle_navigation_message(window, state_pointer, &message) {
            continue;
        }
        if unsafe { IsDialogMessageW(window, &message) } == 0 {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    let state = unsafe { Box::from_raw(state_pointer) };
    Ok(state.result)
}

fn register_window_class() -> anyhow::Result<()> {
    let class = wide(WINDOW_CLASS);
    let instance = unsafe { GetModuleHandleW(null()) };
    let mut window_class: WNDCLASSEXW = unsafe { std::mem::zeroed() };
    window_class.cbSize = size_of::<WNDCLASSEXW>() as u32;
    window_class.style = CS_HREDRAW | CS_VREDRAW;
    window_class.lpfnWndProc = Some(window_proc);
    window_class.hInstance = instance;
    window_class.hCursor = unsafe { LoadCursorW(null_mut(), IDC_ARROW) };
    window_class.hbrBackground = (COLOR_WINDOW + 1) as usize as _;
    window_class.lpszClassName = class.as_ptr();
    let atom = unsafe { RegisterClassExW(&window_class) };
    if atom == 0 {
        let error = unsafe { GetLastError() };
        // ERROR_CLASS_ALREADY_EXISTS
        ensure!(
            error == 1410,
            "画面クラスを登録できません: Win32 error {error}"
        );
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
        unsafe {
            SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
        }
    }
    let state_pointer = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) as *mut UiState };

    match message {
        WM_CREATE => {
            if state_pointer.is_null() {
                return -1;
            }
            unsafe { (*state_pointer).background_brush = CreateSolidBrush(APP_BACKGROUND) };
            if unsafe { create_controls(window, &mut *state_pointer) }.is_err() {
                return -1;
            }
            0
        }
        WM_ERASEBKGND if !state_pointer.is_null() => {
            let mut rect: RECT = unsafe { std::mem::zeroed() };
            unsafe {
                GetClientRect(window, &mut rect);
                FillRect(wparam as _, &rect, (*state_pointer).background_brush);
            }
            1
        }
        WM_CTLCOLORSTATIC if !state_pointer.is_null() => unsafe {
            SetBkColor(wparam as _, APP_BACKGROUND);
            SetTextColor(wparam as _, APP_TEXT);
            (*state_pointer).background_brush as isize
        },
        WM_COMMAND => {
            if state_pointer.is_null() {
                return 0;
            }
            let state = unsafe { &mut *state_pointer };
            let id = low_word(wparam) as i32;
            let notification = high_word(wparam);
            if id == ID_SEARCH && notification == EN_CHANGE as u16 {
                unsafe { apply_search(state) };
            } else if id == ID_SETTINGS && notification == BN_CLICKED as u16 {
                state.result = SelectorResult::Settings {
                    url: state.url.clone(),
                };
                unsafe { DestroyWindow(window) };
            } else if id == ID_CLOSE {
                if notification == BN_PUSHED as u16 {
                    state.close_started = Some(Instant::now());
                    unsafe { SetTimer(window, TIMER_HOLD, 50, None) };
                } else if notification == BN_UNPUSHED as u16 {
                    state.close_started = None;
                }
            } else if notification == BN_CLICKED as u16
                && ((ID_PROFILE_BASE..ID_PROFILE_BASE + MAX_TILES as i32).contains(&id)
                    || (ID_PROFILE_ICON_BASE..ID_PROFILE_ICON_BASE + MAX_TILES as i32)
                        .contains(&id))
            {
                let button_index = if id >= ID_PROFILE_ICON_BASE {
                    (id - ID_PROFILE_ICON_BASE) as usize
                } else {
                    (id - ID_PROFILE_BASE) as usize
                };
                if let Some(profile_index) = state
                    .filtered_indices
                    .get(button_index)
                    .and_then(|value| *value)
                {
                    state.result = SelectorResult::Profile {
                        profile_id: state.profiles[profile_index].id,
                        url: state.url.clone(),
                    };
                    unsafe { DestroyWindow(window) };
                }
            }
            0
        }
        WM_TIMER if wparam == TIMER_HOLD => {
            if !state_pointer.is_null() {
                let state = unsafe { &mut *state_pointer };
                let close_elapsed = state
                    .close_started
                    .is_some_and(|started| started.elapsed() >= HOLD_DURATION);
                let escape_elapsed = state
                    .escape_started
                    .is_some_and(|started| started.elapsed() >= HOLD_DURATION);
                if close_elapsed || escape_elapsed {
                    state.result = SelectorResult::Cancelled;
                    unsafe { DestroyWindow(window) };
                }
            }
            0
        }
        WM_COPYDATA => {
            if state_pointer.is_null() {
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
            let Ok(url) = ValidatedUrl::parse(value) else {
                return 0;
            };
            let state = unsafe { &mut *state_pointer };
            state.url = Some(url);
            unsafe { refresh_url(state) };
            unsafe {
                ShowWindow(window, SW_RESTORE);
                SetForegroundWindow(window);
            }
            1
        }
        WM_ACTIVATE => {
            if low_word(wparam) != WA_INACTIVE as u16 {
                unsafe { SetWindowPos(window, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE) };
            }
            0
        }
        WM_CLOSE => 0,
        WM_DESTROY => {
            if !state_pointer.is_null() {
                for bitmap in unsafe { (*state_pointer).loaded_bitmaps.values() } {
                    unsafe { DeleteObject((*bitmap).cast()) };
                }
                unsafe { DeleteObject((*state_pointer).background_brush) };
            }
            unsafe {
                KillTimer(window, TIMER_HOLD);
                PostQuitMessage(0);
            }
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
unsafe fn create_controls(window: HWND, state: &mut UiState) -> anyhow::Result<()> {
    let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
    create_static(window, "◎ HttpSelector", 24, 10, 250, 32, font);
    create_static(
        window,
        "リンクを開くプロファイルを選択してください",
        280,
        16,
        360,
        24,
        font,
    );
    create_static(
        window,
        "ESC / 閉じる は3秒長押しで終了",
        650,
        16,
        270,
        24,
        font,
    );
    state.close_button = create_button(
        window,
        "× 3秒長押し",
        ID_CLOSE,
        1000,
        8,
        130,
        34,
        (BS_PUSHBUTTON | BS_NOTIFY) as u32,
        font,
    );

    create_static(window, "↗ 受信したリンク", 24, 53, 180, 22, font);
    let original = state
        .url
        .as_ref()
        .map(ValidatedUrl::original)
        .unwrap_or("URLが指定されていません。設定からプロファイルを確認できます。");
    state.original_edit = create_readonly_edit(window, original, 24, 75, 1112, 56, font);
    create_static(
        window,
        "⌁ 読みやすい表示（確認専用）",
        24,
        137,
        260,
        22,
        font,
    );
    let preview = state
        .url
        .as_ref()
        .map(ValidatedUrl::decoded_preview)
        .unwrap_or("");
    state.preview_edit = create_readonly_edit(window, preview, 24, 159, 1112, 50, font);

    create_static(window, "⌕ 検索", 24, 220, 70, 28, font);
    state.search = create_edit(window, "", ID_SEARCH, 94, 216, 804, 32, false, font);
    create_button(
        window,
        "⚙ 設定・再スキャン",
        ID_SETTINGS,
        914,
        216,
        222,
        32,
        BS_PUSHBUTTON as u32,
        font,
    );

    create_static(
        window,
        "★ すぐに開く（お気に入り・最大5件）",
        24,
        260,
        320,
        24,
        font,
    );
    create_static(
        window,
        "◫ すべてのプロファイル（最大20件）",
        24,
        370,
        320,
        24,
        font,
    );

    for index in 0..MAX_TILES {
        let (x, y) = tile_position(index);
        let button = create_button(
            window,
            "",
            ID_PROFILE_BASE + index as i32,
            x,
            y,
            204,
            88,
            (BS_PUSHBUTTON | BS_MULTILINE) as u32 | WS_CLIPCHILDREN,
            font,
        );
        state.profile_buttons.push(button);
        let image = create_control(
            window,
            WC_BUTTONW,
            "",
            ID_PROFILE_ICON_BASE + index as i32,
            x + 8,
            y + 28,
            32,
            32,
            WS_CHILD | BS_BITMAP as u32,
            font,
        );
        state.profile_images.push(image);
    }
    unsafe { refresh_tiles(state) };
    unsafe { SetFocus(state.search) };
    Ok(())
}

unsafe fn refresh_url(state: &mut UiState) {
    let original = state
        .url
        .as_ref()
        .map(ValidatedUrl::original)
        .unwrap_or("URLが指定されていません");
    let preview = state
        .url
        .as_ref()
        .map(ValidatedUrl::decoded_preview)
        .unwrap_or("");
    let original = wide(original);
    let preview = wide(preview);
    unsafe {
        SetWindowTextW(state.original_edit, original.as_ptr());
        SetWindowTextW(state.preview_edit, preview.as_ptr());
        refresh_tiles(state);
    }
}

unsafe fn apply_search(state: &mut UiState) {
    let query = unsafe { window_text(state.search) };
    state.filtered_indices = filtered_slots(&state.profiles, &query);
    unsafe { refresh_tiles(state) };
}

unsafe fn refresh_tiles(state: &mut UiState) {
    for button_index in 0..state.profile_buttons.len() {
        let button = state.profile_buttons[button_index];
        let image = state.profile_images[button_index];
        if let Some(profile_index) = state
            .filtered_indices
            .get(button_index)
            .and_then(|value| *value)
        {
            let (favorite, display_name, comment, glyph, icon_path) = {
                let profile = &state.profiles[profile_index];
                (
                    if profile.favorite { "★ " } else { "" },
                    profile.display_name().to_owned(),
                    if profile.comment.trim().is_empty() {
                        profile.browser_type.display_name().to_owned()
                    } else {
                        profile.comment.clone()
                    },
                    profile.browser_type.icon_glyph(),
                    profile.icon_path.clone(),
                )
            };
            let bitmap = icon_path
                .as_deref()
                .and_then(|path| load_cached_bitmap(state, path));
            let text = if bitmap.is_some() {
                format!("{favorite}      {display_name}\r\n{comment}")
            } else {
                format!("{favorite}{glyph} {display_name}\r\n{comment}")
            };
            let text = wide(&text);
            unsafe {
                SetWindowTextW(button, text.as_ptr());
                let (x, y) = tile_position(button_index);
                SetWindowPos(
                    button,
                    null_mut(),
                    x,
                    y,
                    204,
                    88,
                    SWP_NOZORDER | SWP_SHOWWINDOW,
                );
                EnableWindow(button, i32::from(state.url.is_some()));
                SendMessageW(
                    image,
                    BM_SETIMAGE,
                    IMAGE_BITMAP as usize,
                    bitmap.unwrap_or(null_mut()) as isize,
                );
                SetWindowPos(image, HWND_TOP, x + 8, y + 28, 32, 32, SWP_SHOWWINDOW);
            }
        } else {
            unsafe {
                ShowWindow(button, SW_HIDE);
                ShowWindow(image, SW_HIDE);
            }
        }
    }
}

fn load_cached_bitmap(state: &mut UiState, path: &str) -> Option<HBITMAP> {
    if let Some(bitmap) = state.loaded_bitmaps.get(path) {
        return Some(*bitmap);
    }
    let path_wide = wide(path);
    let bitmap = unsafe {
        LoadImageW(
            null_mut(),
            path_wide.as_ptr(),
            IMAGE_BITMAP,
            32,
            32,
            LR_LOADFROMFILE,
        )
    };
    if bitmap.is_null() {
        return None;
    }
    let bitmap = bitmap.cast();
    state.loaded_bitmaps.insert(path.to_owned(), bitmap);
    Some(bitmap)
}

fn filtered_slots(profiles: &[Profile], query: &str) -> Vec<Option<usize>> {
    let mut slots = vec![None; MAX_TILES];
    for (slot, (profile_index, _)) in profiles
        .iter()
        .enumerate()
        .filter(|(_, profile)| profile.favorite && profile.matches(query))
        .take(5)
        .enumerate()
    {
        slots[slot] = Some(profile_index);
    }
    for (offset, (profile_index, _)) in profiles
        .iter()
        .enumerate()
        .filter(|(_, profile)| !profile.favorite && profile.matches(query))
        .take(20)
        .enumerate()
    {
        slots[5 + offset] = Some(profile_index);
    }
    slots
}

fn tile_position(index: usize) -> (i32, i32) {
    let column = (index % 5) as i32;
    if index < 5 {
        (24 + column * 216, 282)
    } else {
        let normal = index - 5;
        let row = (normal / 5) as i32;
        let column = (normal % 5) as i32;
        (24 + column * 216, 396 + row * 98)
    }
}

fn handle_navigation_message(window: HWND, state_pointer: *mut UiState, message: &MSG) -> bool {
    if state_pointer.is_null() {
        return false;
    }
    let state = unsafe { &mut *state_pointer };
    if message.message == WM_KEYDOWN && message.wParam == VK_ESCAPE as usize {
        if state.escape_started.is_none() {
            state.escape_started = Some(Instant::now());
            unsafe { SetTimer(window, TIMER_HOLD, 50, None) };
        }
        return true;
    }
    if message.message == WM_KEYUP && message.wParam == VK_ESCAPE as usize {
        state.escape_started = None;
        return true;
    }
    if message.message == WM_KEYDOWN {
        let key = message.wParam as u32;
        if [VK_LEFT, VK_RIGHT, VK_UP, VK_DOWN].contains(&(key as u16)) {
            return move_tile_focus(state, key);
        }
        if key == VK_RETURN as u32 {
            let focus = unsafe { GetFocus() };
            if let Some(index) = state
                .profile_buttons
                .iter()
                .position(|button| *button == focus)
            {
                unsafe {
                    SendMessageW(
                        window,
                        WM_COMMAND,
                        (ID_PROFILE_BASE + index as i32) as usize,
                        focus as isize,
                    );
                }
                return true;
            }
        }
    }
    false
}

fn move_tile_focus(state: &UiState, key: u32) -> bool {
    if state.filtered_indices.iter().all(Option::is_none) {
        return false;
    }
    let focus = unsafe { GetFocus() };
    let Some(current) = state
        .profile_buttons
        .iter()
        .position(|button| *button == focus)
    else {
        return false;
    };
    let target = if key == VK_LEFT as u32 {
        current.saturating_sub(1)
    } else if key == VK_RIGHT as u32 {
        (current + 1).min(MAX_TILES - 1)
    } else if key == VK_UP as u32 {
        current.saturating_sub(5)
    } else if key == VK_DOWN as u32 {
        (current + 5).min(MAX_TILES - 1)
    } else {
        current
    };
    if state.filtered_indices[target].is_some() {
        unsafe { SetFocus(state.profile_buttons[target]) };
    }
    true
}

fn centered_on_cursor_monitor(width: i32, height: i32) -> (i32, i32) {
    let mut cursor = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut cursor) };
    let monitor = unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST) };
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = size_of::<MONITORINFO>() as u32;
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return (CW_USEDEFAULT, CW_USEDEFAULT);
    }
    let work = info.rcWork;
    (
        work.left + ((work.right - work.left - width) / 2).max(0),
        work.top + ((work.bottom - work.top - height) / 2).max(0),
    )
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
            wide("STATIC").as_ptr(),
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

unsafe fn create_readonly_edit(
    parent: HWND,
    text: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    font: *mut core::ffi::c_void,
) -> HWND {
    unsafe { create_edit(parent, text, 0, x, y, width, height, true, font) }
}

#[allow(clippy::too_many_arguments)]
unsafe fn create_edit(
    parent: HWND,
    text: &str,
    id: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    readonly: bool,
    font: *mut core::ffi::c_void,
) -> HWND {
    let mut style = WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER | ES_LEFT as u32;
    if readonly {
        style |= ES_READONLY as u32 | ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32;
    } else {
        style |= ES_AUTOHSCROLL as u32;
    }
    unsafe {
        create_control(
            parent,
            wide("EDIT").as_ptr(),
            text,
            id,
            x,
            y,
            width,
            height,
            style,
            font,
        )
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn create_button(
    parent: HWND,
    text: &str,
    id: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    button_style: u32,
    font: *mut core::ffi::c_void,
) -> HWND {
    unsafe {
        create_control(
            parent,
            WC_BUTTONW,
            text,
            id,
            x,
            y,
            width,
            height,
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | button_style,
            font,
        )
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn create_control(
    parent: HWND,
    class: *const u16,
    text: &str,
    id: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    style: u32,
    font: *mut core::ffi::c_void,
) -> HWND {
    let text = wide(text);
    let control = unsafe {
        CreateWindowExW(
            0,
            class,
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

pub fn show_error(title: &str, message: &str) {
    let title = wide(title);
    let message = wide(message);
    unsafe {
        MessageBoxW(
            null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND,
        );
    }
}

pub fn show_information(title: &str, message: &str) {
    let title = wide(title);
    let message = wide(message);
    unsafe {
        MessageBoxW(
            null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND,
        );
    }
}

pub fn ask_integrity_recovery(has_valid_backup: bool) -> IntegrityRecoveryAction {
    let title = wide("HttpSelector - 設定ファイルエラー");
    if has_valid_backup {
        let message = wide(
            "設定ファイルが変更または破損しています。\r\n\r\n「はい」: バックアップから復旧\r\n「いいえ」: 破損ファイルを退避して初期化\r\n「キャンセル」: 終了",
        );
        match unsafe {
            MessageBoxW(
                null_mut(),
                message.as_ptr(),
                title.as_ptr(),
                MB_YESNOCANCEL | MB_ICONWARNING | MB_SETFOREGROUND,
            )
        } {
            IDYES => IntegrityRecoveryAction::Restore,
            IDNO => IntegrityRecoveryAction::Initialize,
            _ => IntegrityRecoveryAction::Exit,
        }
    } else {
        let message = wide(
            "設定ファイルが変更または破損しており、有効なバックアップもありません。\r\n\r\n「OK」: 破損ファイルを退避して初期化\r\n「キャンセル」: 終了",
        );
        match unsafe {
            MessageBoxW(
                null_mut(),
                message.as_ptr(),
                title.as_ptr(),
                MB_OKCANCEL | MB_ICONWARNING | MB_SETFOREGROUND,
            )
        } {
            IDOK => IntegrityRecoveryAction::Initialize,
            _ => IntegrityRecoveryAction::Exit,
        }
    }
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

#[allow(dead_code)]
fn _rect_width(rect: RECT) -> i32 {
    rect.right - rect.left
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::BrowserType;

    fn profile(id: i64, favorite: bool) -> Profile {
        Profile {
            id,
            browser_type: BrowserType::Edge,
            profile_id: format!("Profile {id}"),
            detected_name: format!("Profile {id}"),
            alias_name: String::new(),
            comment: String::new(),
            sort_order: id,
            favorite,
            enabled: true,
            available: true,
            icon_path: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn normal_profiles_start_in_normal_area_even_with_few_favorites() {
        let profiles = vec![profile(1, true), profile(2, false), profile(3, false)];
        let slots = filtered_slots(&profiles, "");
        assert_eq!(slots[0], Some(0));
        assert_eq!(slots[1], None);
        assert_eq!(slots[5], Some(1));
        assert_eq!(slots[6], Some(2));
    }
}
