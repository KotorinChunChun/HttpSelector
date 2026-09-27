use crate::app_title;
use crate::model::{DetectedProfile, Profile};
use crate::profile_scan::BackgroundProfileScan;
use crate::storage::{MAX_ESCAPE_HOLD_DURATION_TENTHS, MIN_ESCAPE_HOLD_DURATION_TENTHS};
use crate::ui_layout::SelectorLayout;
use crate::ui_support::{self as common, Page, Theme};
use crate::url_input::ValidatedUrl;
use anyhow::ensure;
use std::collections::HashMap;
use std::mem::size_of;
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{DRAWITEMSTRUCT, ODS_DISABLED, ODS_FOCUS, ODS_SELECTED};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetFocus, SetFocus, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_RETURN, VK_RIGHT, VK_UP,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const WINDOW_CLASS: &str = "HttpSelector.MainWindow";
const MAX_TILES: usize = 25;
const TIMER_HOLD: usize = 1;
const TIMER_PROFILE_SCAN: usize = 2;
const WM_URL_RECEIVED: u32 = WM_APP + 1;
const ID_SETTINGS: i32 = 101;
const ID_CLOSE: i32 = 102;
const ID_WINDOWS_SETTINGS: i32 = 103;
const ID_DEFAULT_RULE: i32 = 104;
const ID_FAVORITE_UP: i32 = 105;
const ID_FAVORITE_DOWN: i32 = 106;
const ID_REFRESH: i32 = 107;
const ID_FAVORITE_PIN: i32 = 108;
const ID_PROFILE_BASE: i32 = 1000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SelectorResult {
    UrlReceived(ValidatedUrl),
    Profile {
        profile_id: i64,
        url: Option<ValidatedUrl>,
    },
    Settings {
        url: Option<ValidatedUrl>,
    },
    ConfigureDefaultRule {
        profile_id: i64,
        url: ValidatedUrl,
    },
    FavoriteChanged {
        profile_id: i64,
        favorite: bool,
        url: Option<ValidatedUrl>,
    },
    RefreshRequested {
        url: Option<ValidatedUrl>,
        selected_profile_id: Option<i64>,
    },
    ProfilesDetected {
        detected: Vec<DetectedProfile>,
        url: Option<ValidatedUrl>,
        selected_profile_id: Option<i64>,
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
    loaded_bitmaps: HashMap<String, HBITMAP>,
    theme: Theme,
    page: Page,
    fixed: Vec<HWND>,
    favorites_heading: HWND,
    normal_heading: HWND,
    favorite_empty: HWND,
    normal_empty: HWND,
    favorite_up: HWND,
    favorite_down: HWND,
    favorite_pin: HWND,
    refresh: HWND,
    default_rule: HWND,
    status: HWND,
    original_edit: HWND,
    preview_edit: HWND,
    escape_hold_duration_tenths: u32,
    open_profile_on_double_click: bool,
    focused_profile: Option<usize>,
    escape_started: Option<Instant>,
    background_scan: Option<BackgroundProfileScan>,
    result: SelectorResult,
}

impl UiState {
    fn new(
        url: Option<ValidatedUrl>,
        profiles: Vec<Profile>,
        escape_hold_duration_tenths: u32,
        open_profile_on_double_click: bool,
        selected_profile_id: Option<i64>,
    ) -> Self {
        let filtered_indices = profile_slots(&profiles);
        let focused_profile = filtered_indices
            .iter()
            .flatten()
            .copied()
            .find(|index| Some(profiles[*index].id) == selected_profile_id)
            .or_else(|| filtered_indices.iter().flatten().next().copied());
        Self {
            url,
            profiles,
            filtered_indices,
            profile_buttons: Vec::new(),
            loaded_bitmaps: HashMap::new(),
            theme: Theme::new(96),
            page: Page::default(),
            fixed: Vec::new(),
            favorites_heading: null_mut(),
            normal_heading: null_mut(),
            favorite_empty: null_mut(),
            normal_empty: null_mut(),
            favorite_up: null_mut(),
            favorite_down: null_mut(),
            favorite_pin: null_mut(),
            refresh: null_mut(),
            default_rule: null_mut(),
            status: null_mut(),
            original_edit: null_mut(),
            preview_edit: null_mut(),
            escape_hold_duration_tenths,
            open_profile_on_double_click,
            focused_profile,
            escape_started: None,
            background_scan: None,
            result: SelectorResult::Cancelled,
        }
    }
}

impl Drop for UiState {
    fn drop(&mut self) {
        for bitmap in self.loaded_bitmaps.values() {
            unsafe {
                DeleteObject(*bitmap);
            }
        }
    }
}

pub fn run_selector(
    url: Option<ValidatedUrl>,
    profiles: Vec<Profile>,
    escape_hold_duration_tenths: u32,
    open_profile_on_double_click: bool,
) -> anyhow::Result<SelectorResult> {
    run_selector_with_selection(
        url,
        profiles,
        escape_hold_duration_tenths,
        open_profile_on_double_click,
        None,
    )
}

pub fn run_selector_with_selection(
    url: Option<ValidatedUrl>,
    profiles: Vec<Profile>,
    escape_hold_duration_tenths: u32,
    open_profile_on_double_click: bool,
    selected_profile_id: Option<i64>,
) -> anyhow::Result<SelectorResult> {
    run_selector_with_selection_and_scan(
        url,
        profiles,
        escape_hold_duration_tenths,
        open_profile_on_double_click,
        selected_profile_id,
        None,
    )
}

pub fn run_selector_with_selection_and_scan(
    url: Option<ValidatedUrl>,
    profiles: Vec<Profile>,
    escape_hold_duration_tenths: u32,
    open_profile_on_double_click: bool,
    selected_profile_id: Option<i64>,
    background_scan: Option<BackgroundProfileScan>,
) -> anyhow::Result<SelectorResult> {
    run_selector_window(
        url,
        profiles,
        escape_hold_duration_tenths,
        open_profile_on_double_click,
        WS_EX_TOOLWINDOW,
        selected_profile_id,
        background_scan,
    )
}

/// 実データを使わないプレビューでは、画面操作ツールから検出できる通常ウィンドウを使う。
#[cfg(debug_assertions)]
pub fn run_selector_preview(
    url: Option<ValidatedUrl>,
    profiles: Vec<Profile>,
    escape_hold_duration_tenths: u32,
    open_profile_on_double_click: bool,
) -> anyhow::Result<SelectorResult> {
    run_selector_window(
        url,
        profiles,
        escape_hold_duration_tenths,
        open_profile_on_double_click,
        WS_EX_APPWINDOW,
        None,
        None,
    )
}

fn run_selector_window(
    url: Option<ValidatedUrl>,
    profiles: Vec<Profile>,
    escape_hold_duration_tenths: u32,
    open_profile_on_double_click: bool,
    window_style: u32,
    selected_profile_id: Option<i64>,
    background_scan: Option<BackgroundProfileScan>,
) -> anyhow::Result<SelectorResult> {
    let style = WS_POPUP | WS_THICKFRAME | WS_CLIPCHILDREN | WS_VSCROLL;
    let ex_style = WS_EX_TOPMOST | window_style | WS_EX_CONTROLPARENT;
    let bounds = common::initial_bounds(1120, 900, style, ex_style);
    register_window_class()?;
    let escape_hold_duration_tenths = escape_hold_duration_tenths.clamp(
        MIN_ESCAPE_HOLD_DURATION_TENTHS,
        MAX_ESCAPE_HOLD_DURATION_TENTHS,
    );
    let mut state = UiState::new(
        url,
        profiles,
        escape_hold_duration_tenths,
        open_profile_on_double_click,
        selected_profile_id,
    );
    state.background_scan = background_scan;
    let state_pointer = Box::into_raw(Box::new(state));
    let window = unsafe {
        CreateWindowExW(
            ex_style,
            wide(&crate::app_identity::AppIdentity::current()?.window_class(WINDOW_CLASS)).as_ptr(),
            wide(&app_title::main()).as_ptr(),
            style,
            bounds.x,
            bounds.y,
            bounds.width,
            bounds.height,
            null_mut(),
            null_mut(),
            GetModuleHandleW(null()),
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
        if common::handle_edit_shortcut(&message) {
            continue;
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
        if message.message == WM_KEYDOWN && unsafe { IsWindow(window) } != 0 {
            let state = unsafe { &mut *state_pointer };
            state
                .page
                .reveal(window, unsafe { GetFocus() }, state.theme.dpi);
        }
    }
    let state = unsafe { Box::from_raw(state_pointer) };
    Ok(state.result.clone())
}

fn register_window_class() -> anyhow::Result<()> {
    let class = wide(&crate::app_identity::AppIdentity::current()?.window_class(WINDOW_CLASS));
    let mut window_class: WNDCLASSEXW = unsafe { std::mem::zeroed() };
    window_class.cbSize = size_of::<WNDCLASSEXW>() as u32;
    window_class.style = CS_HREDRAW | CS_VREDRAW;
    window_class.lpfnWndProc = Some(window_proc);
    window_class.hInstance = unsafe { GetModuleHandleW(null()) };
    window_class.hCursor = unsafe { LoadCursorW(null_mut(), IDC_ARROW) };
    window_class.lpszClassName = class.as_ptr();
    if unsafe { RegisterClassExW(&window_class) } == 0 {
        let error = unsafe { GetLastError() };
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
    let pointer = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) as *mut UiState };
    match message {
        WM_SYSCOMMAND if common::is_maximize_command(wparam) => 0,
        WM_CREATE if !pointer.is_null() => {
            let state = unsafe { &mut *pointer };
            state.theme = Theme::new(unsafe { GetDpiForWindow(window) }.max(96));
            create_controls(window, state);
            if state.background_scan.is_some() {
                unsafe {
                    SetTimer(window, TIMER_PROFILE_SCAN, 100, None);
                }
            }
            0
        }
        WM_SIZE if !pointer.is_null() => {
            let state = unsafe { &mut *pointer };
            if !state.fixed.is_empty() {
                layout_controls(window, state);
            }
            0
        }
        WM_DPICHANGED if !pointer.is_null() => {
            let state = unsafe { &mut *pointer };
            let theme = Theme::new(low_word(wparam) as u32);
            common::update_fonts(&state.page, &state.theme, &theme);
            for button in &state.profile_buttons {
                unsafe {
                    SendMessageW(*button, WM_SETFONT, theme.font as usize, 1);
                }
            }
            state.theme = theme;
            let bounds = unsafe { &*(lparam as *const RECT) };
            unsafe {
                SetWindowPos(
                    window,
                    null_mut(),
                    bounds.left,
                    bounds.top,
                    bounds.right - bounds.left,
                    bounds.bottom - bounds.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            0
        }
        WM_GETMINMAXINFO => {
            let info = unsafe { &mut *(lparam as *mut MINMAXINFO) };
            let dpi = unsafe { GetDpiForWindow(window) }.max(96);
            info.ptMinTrackSize.x = common::scale(400, dpi).min(info.ptMaxTrackSize.x);
            info.ptMinTrackSize.y = common::scale(320, dpi).min(info.ptMaxTrackSize.y);
            0
        }
        WM_NCHITTEST => {
            let hit = unsafe { DefWindowProcW(window, message, wparam, lparam) };
            if hit == HTCLIENT as isize && !pointer.is_null() {
                let mut point = POINT {
                    x: low_word(lparam as usize) as i16 as i32,
                    y: high_word(lparam as usize) as i16 as i32,
                };
                unsafe {
                    ScreenToClient(window, &mut point);
                }
                let state = unsafe { &*pointer };
                if point.y + state.page.offset < common::scale(64, state.theme.dpi) {
                    return HTCAPTION as isize;
                }
            }
            hit
        }
        WM_VSCROLL | WM_MOUSEWHEEL if !pointer.is_null() => {
            let state = unsafe { &mut *pointer };
            state.page.scroll(window, message, wparam, state.theme.dpi);
            0
        }
        WM_ERASEBKGND if !pointer.is_null() => {
            let mut rect: RECT = unsafe { std::mem::zeroed() };
            unsafe {
                GetClientRect(window, &mut rect);
                FillRect(wparam as _, &rect, (*pointer).theme.background);
            }
            1
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORBTN if !pointer.is_null() => {
            let state = unsafe { &*pointer };
            let field = lparam as HWND == state.original_edit
                || lparam as HWND == state.preview_edit
                || message == WM_CTLCOLOREDIT;
            unsafe {
                SetBkColor(
                    wparam as _,
                    if field {
                        0x00FFFFFF
                    } else {
                        common::BACKGROUND
                    },
                );
                SetTextColor(wparam as _, common::TEXT);
            }
            if field {
                state.theme.surface as isize
            } else {
                state.theme.background as isize
            }
        }
        WM_DRAWITEM if !pointer.is_null() => {
            let item = unsafe { &*(lparam as *const DRAWITEMSTRUCT) };
            let state = unsafe { &*pointer };
            if (ID_PROFILE_BASE as u32..ID_PROFILE_BASE as u32 + MAX_TILES as u32)
                .contains(&item.CtlID)
            {
                draw_tile(state, item);
            } else {
                common::draw_button(item);
            }
            1
        }
        WM_COMMAND if !pointer.is_null() => {
            let state = unsafe { &mut *pointer };
            let id = low_word(wparam) as i32;
            let notification = high_word(wparam);
            if id == ID_SETTINGS && notification == BN_CLICKED as u16 {
                common::release_topmost(window);
                state.result = SelectorResult::Settings {
                    url: state.url.clone(),
                };
                unsafe {
                    DestroyWindow(window);
                }
            } else if id == ID_WINDOWS_SETTINGS && notification == BN_CLICKED as u16 {
                open_windows_default_apps(window);
            } else if id == ID_REFRESH && notification == BN_CLICKED as u16 {
                state.result = SelectorResult::RefreshRequested {
                    url: state.url.clone(),
                    selected_profile_id: focused_profile_id(state),
                };
                unsafe {
                    DestroyWindow(window);
                }
            } else if id == ID_DEFAULT_RULE && notification == BN_CLICKED as u16 {
                let Some(profile_index) = state.focused_profile else {
                    show_information_for(
                        window,
                        &app_title::main(),
                        "先にプロファイルをクリックしてフォーカスしてください。",
                    );
                    return 0;
                };
                let Some(url) = state.url.clone() else {
                    show_information_for(
                        window,
                        &app_title::main(),
                        "URLを受け取っているときだけ既定ルールを登録できます。",
                    );
                    return 0;
                };
                state.result = SelectorResult::ConfigureDefaultRule {
                    profile_id: state.profiles[profile_index].id,
                    url,
                };
                common::release_topmost(window);
                unsafe {
                    DestroyWindow(window);
                }
            } else if (id == ID_FAVORITE_UP || id == ID_FAVORITE_DOWN || id == ID_FAVORITE_PIN)
                && notification == BN_CLICKED as u16
            {
                let Some(profile_index) = state.focused_profile else {
                    show_information_for(
                        window,
                        &app_title::main(),
                        "先にプロファイルをクリックしてフォーカスしてください。",
                    );
                    return 0;
                };
                let favorite = if id == ID_FAVORITE_PIN {
                    !state.profiles[profile_index].favorite
                } else {
                    id == ID_FAVORITE_UP
                };
                if state.profiles[profile_index].favorite == favorite {
                    return 0;
                }
                state.result = SelectorResult::FavoriteChanged {
                    profile_id: state.profiles[profile_index].id,
                    favorite,
                    url: state.url.clone(),
                };
                unsafe {
                    DestroyWindow(window);
                }
            } else if id == ID_CLOSE && notification == BN_CLICKED as u16 {
                state.result = SelectorResult::Cancelled;
                unsafe {
                    DestroyWindow(window);
                }
            } else if [
                BN_CLICKED as u16,
                BN_DOUBLECLICKED as u16,
                BN_SETFOCUS as u16,
            ]
            .contains(&notification)
                && (ID_PROFILE_BASE..ID_PROFILE_BASE + MAX_TILES as i32).contains(&id)
            {
                if let Some(index) = state.filtered_indices[(id - ID_PROFILE_BASE) as usize] {
                    state.focused_profile = Some(index);
                    let should_open = if state.open_profile_on_double_click {
                        notification == BN_DOUBLECLICKED as u16
                    } else {
                        notification == BN_CLICKED as u16
                    };
                    if should_open && state.url.is_some() {
                        state.result = SelectorResult::Profile {
                            profile_id: state.profiles[index].id,
                            url: state.url.clone(),
                        };
                        common::release_topmost(window);
                        unsafe {
                            DestroyWindow(window);
                        }
                    } else {
                        refresh_selection_visuals(state);
                    }
                }
            }
            0
        }
        WM_TIMER if wparam == TIMER_PROFILE_SCAN && !pointer.is_null() => {
            let state = unsafe { &mut *pointer };
            let Some(scan) = state.background_scan.as_ref() else {
                unsafe {
                    KillTimer(window, TIMER_PROFILE_SCAN);
                }
                return 0;
            };
            let completed = scan.try_take();
            if completed.is_none() && !scan.is_finished() {
                return 0;
            }
            unsafe {
                KillTimer(window, TIMER_PROFILE_SCAN);
            }
            state.background_scan = None;
            match completed {
                Some(Ok(detected)) => {
                    if crate::profile_scan::changes(&state.profiles, &detected).changed() {
                        state.result = SelectorResult::ProfilesDetected {
                            detected,
                            url: state.url.clone(),
                            selected_profile_id: focused_profile_id(state),
                        };
                        unsafe {
                            DestroyWindow(window);
                        }
                    }
                }
                Some(Err(_)) | None => {
                    common::set_text(
                        state.status,
                        "プロファイルの自動更新に失敗しました。更新ボタンから再試行できます。",
                    );
                }
            }
            0
        }
        WM_TIMER if wparam == TIMER_HOLD && !pointer.is_null() => {
            let state = unsafe { &mut *pointer };
            if let Some(started) = state.escape_started {
                let elapsed = started.elapsed();
                let hold_duration =
                    Duration::from_millis(state.escape_hold_duration_tenths as u64 * 100);
                if elapsed >= hold_duration {
                    state.result = SelectorResult::Cancelled;
                    unsafe {
                        DestroyWindow(window);
                    }
                } else {
                    common::set_text(
                        state.status,
                        &format!(
                            "終了まで {:.1} 秒（離すと取り消し）",
                            hold_duration
                                .checked_sub(elapsed)
                                .unwrap_or_default()
                                .as_secs_f32()
                        ),
                    );
                }
            }
            0
        }
        WM_COPYDATA if !pointer.is_null() => {
            let Some(url) = (unsafe { crate::single_instance::received_url(lparam) }) else {
                return 0;
            };
            let state = unsafe { &mut *pointer };
            state.url = Some(url);
            refresh_url(window, state);
            // 同期IPCへの応答後に画面を閉じ、通常起動と同じ既定ルール評価へ戻す。
            unsafe { PostMessageW(window, WM_URL_RECEIVED, 0, 0) };
            unsafe {
                ShowWindow(window, SW_RESTORE);
                SetForegroundWindow(window);
            }
            1
        }
        WM_URL_RECEIVED if !pointer.is_null() => {
            let state = unsafe { &mut *pointer };
            if let Some(url) = state.url.clone() {
                state.result = SelectorResult::UrlReceived(url);
                unsafe { DestroyWindow(window) };
            }
            0
        }
        WM_ACTIVATE if !pointer.is_null() => {
            let state = unsafe { &mut *pointer };
            if low_word(wparam) == WA_INACTIVE as u16 {
                cancel_escape(window, state);
            }
            0
        }
        WM_CLOSE => 0,
        WM_DESTROY => {
            unsafe {
                KillTimer(window, TIMER_HOLD);
                KillTimer(window, TIMER_PROFILE_SCAN);
                PostQuitMessage(0);
            }
            0
        }
        WM_NCDESTROY => unsafe {
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
            DefWindowProcW(window, message, wparam, lparam)
        },
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

fn focused_profile_id(state: &UiState) -> Option<i64> {
    state.focused_profile.map(|index| state.profiles[index].id)
}

fn create_controls(window: HWND, state: &mut UiState) {
    let font = state.theme.font;
    let title = common::label(window, &app_title::main(), state.theme.title);
    let close = common::button(window, "閉じる(ESC長押)", ID_CLOSE, font);
    state.status = common::label(window, "", font);
    let original_label = common::label(window, "受信したURL", state.theme.bold);
    state.original_edit = common::edit(window, 0, true, true, font);
    let preview_label = common::label(window, "デコード表示（確認専用）", state.theme.bold);
    state.preview_edit = common::edit(window, 0, true, true, font);
    let settings = common::button(window, "設定", ID_SETTINGS, font);
    let windows_settings =
        common::button(window, "既定のアプリ設定を開く", ID_WINDOWS_SETTINGS, font);
    state.refresh = common::button(window, "更新", ID_REFRESH, font);
    state.default_rule = common::button(
        window,
        "このプロファイルを既定に設定",
        ID_DEFAULT_RULE,
        font,
    );
    state.fixed = vec![
        title,
        close,
        state.status,
        original_label,
        state.original_edit,
        preview_label,
        state.preview_edit,
        settings,
        windows_settings,
    ];
    state.favorites_heading = common::label(window, "", state.theme.bold);
    state.normal_heading = common::label(window, "", state.theme.bold);
    state.favorite_empty = common::label(window, "", font);
    state.normal_empty = common::label(window, "", font);
    state.favorite_up = common::button(window, "↑", ID_FAVORITE_UP, font);
    state.favorite_down = common::button(window, "↓", ID_FAVORITE_DOWN, font);
    state.favorite_pin = common::button(window, "ピン止め", ID_FAVORITE_PIN, font);
    for index in 0..MAX_TILES {
        state.profile_buttons.push(common::control(
            window,
            "BUTTON",
            "",
            ID_PROFILE_BASE + index as i32,
            WS_TABSTOP | BS_OWNERDRAW as u32 | BS_NOTIFY as u32,
            font,
        ));
    }
    // アイコンは単一ボタン内へ描画し、子ボタンを重ねない。
    let paths: Vec<_> = state
        .profiles
        .iter()
        .filter_map(|profile| profile.icon_path.clone())
        .collect();
    for path in paths {
        state
            .loaded_bitmaps
            .entry(path.clone())
            .or_insert_with(|| unsafe {
                LoadImageW(
                    null_mut(),
                    wide(&path).as_ptr(),
                    IMAGE_BITMAP,
                    32,
                    32,
                    LR_LOADFROMFILE,
                )
                .cast()
            });
    }
    refresh_url(window, state);
    let first_profile = state
        .filtered_indices
        .iter()
        .position(|index| index.is_some() && *index == state.focused_profile);
    refresh_selection_visuals(state);
    let focus = first_profile
        .map(|index| state.profile_buttons[index])
        .or_else(|| state.fixed.get(7).copied())
        .unwrap_or(window);
    unsafe {
        SetFocus(focus);
    }
}

fn open_windows_default_apps(window: HWND) {
    common::release_topmost(window);
    let operation = wide("open");
    let target = wide("ms-settings:defaultapps");
    let result = unsafe {
        ShellExecuteW(
            window,
            operation.as_ptr(),
            target.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        )
    };
    if result as usize <= 32 {
        show_error_for(
            window,
            &app_title::main(),
            "Windowsの既定アプリ設定を開けませんでした。",
        );
    }
}

fn layout_controls(window: HWND, state: &mut UiState) {
    let width = Page::width(window, state.theme.dpi).max(360);
    state.page.controls.clear();
    let layout = SelectorLayout::new(width, &state.filtered_indices);
    for (control, rect) in state.fixed.iter().zip(&layout.fixed) {
        state.page.place(*control, *rect);
    }
    state
        .page
        .place(state.favorites_heading, layout.favorites_heading);
    state.page.place(state.favorite_up, layout.favorite_up);
    state.page.place(state.favorite_down, layout.favorite_down);
    state.page.place(state.favorite_pin, layout.favorite_pin);
    state.page.place(state.refresh, layout.refresh);
    state.page.place(state.default_rule, layout.default_rule);
    state
        .page
        .place(state.normal_heading, layout.normal_heading);
    state
        .page
        .place(state.favorite_empty, layout.favorite_empty);
    state.page.place(state.normal_empty, layout.normal_empty);
    for (index, rect) in layout.tiles.iter().enumerate() {
        if let Some(rect) = rect {
            state.page.place(state.profile_buttons[index], *rect);
        }
    }
    state.page.height = layout.height;
    state.page.apply(window, state.theme.dpi);
}

fn refresh_status(state: &UiState) {
    let status = if state.url.is_some() {
        "プロファイルを選択してください".to_owned()
    } else {
        "URL未指定：設定でプロファイルを管理".to_owned()
    };
    common::set_text(state.status, &status);
}

fn refresh_url(window: HWND, state: &mut UiState) {
    common::set_text(
        state.original_edit,
        state
            .url
            .as_ref()
            .map(ValidatedUrl::original)
            .unwrap_or("URLが指定されていません"),
    );
    common::set_text(
        state.preview_edit,
        state
            .url
            .as_ref()
            .map(ValidatedUrl::decoded_preview)
            .unwrap_or(""),
    );
    refresh_status(state);
    refresh_tiles(window, state);
}

fn refresh_tiles(window: HWND, state: &mut UiState) {
    let favorites = state.filtered_indices[..5].iter().flatten().count();
    let normal = state.filtered_indices[5..].iter().flatten().count();
    common::set_text(
        state.favorites_heading,
        &format!("お気に入り  {favorites} / 5"),
    );
    common::set_text(
        state.normal_heading,
        &format!("プロファイル  {normal} / 20"),
    );
    common::set_text(
        state.favorite_empty,
        "ピン止めしたプロファイルがここに表示されます。",
    );
    common::set_text(
        state.normal_empty,
        "表示するプロファイルはありません。設定から確認できます。",
    );
    unsafe {
        ShowWindow(
            state.favorite_empty,
            if favorites == 0 { SW_SHOW } else { SW_HIDE },
        );
        ShowWindow(
            state.normal_empty,
            if normal == 0 { SW_SHOW } else { SW_HIDE },
        );
    }
    for (slot, button) in state.profile_buttons.iter().enumerate() {
        let visible = state.filtered_indices[slot].is_some();
        if let Some(index) = state.filtered_indices[slot] {
            let profile = &state.profiles[index];
            common::set_text(
                *button,
                &format!(
                    "{} | {} | {}",
                    profile.display_name(),
                    profile.browser_type.display_name(),
                    profile.comment
                ),
            );
        }
        unsafe {
            EnableWindow(*button, i32::from(visible));
            ShowWindow(*button, if visible { SW_SHOW } else { SW_HIDE });
            InvalidateRect(*button, null(), 1);
        }
    }
    refresh_selection_visuals(state);
    layout_controls(window, state);
}

fn refresh_selection_visuals(state: &UiState) {
    let focused = state
        .focused_profile
        .and_then(|index| state.profiles.get(index));
    let favorite_count = state
        .profiles
        .iter()
        .filter(|profile| profile.favorite)
        .count();
    common::set_text(
        state.favorite_pin,
        if focused.is_some_and(|profile| profile.favorite) {
            "ピン解除"
        } else {
            "ピン止め"
        },
    );
    unsafe {
        EnableWindow(
            state.favorite_pin,
            i32::from(
                focused.is_some()
                    && (focused.is_some_and(|profile| profile.favorite) || favorite_count < 5),
            ),
        );
        EnableWindow(
            state.favorite_up,
            i32::from(focused.is_some_and(|profile| !profile.favorite) && favorite_count < 5),
        );
        EnableWindow(
            state.favorite_down,
            i32::from(focused.is_some_and(|profile| profile.favorite)),
        );
        for button in &state.profile_buttons {
            InvalidateRect(*button, null(), 1);
        }
    }
}

fn draw_tile(state: &UiState, item: &DRAWITEMSTRUCT) {
    let Some(slot) = item
        .CtlID
        .checked_sub(ID_PROFILE_BASE as u32)
        .map(|index| index as usize)
    else {
        return;
    };
    let Some(index) = state.filtered_indices.get(slot).copied().flatten() else {
        return;
    };
    let profile = &state.profiles[index];
    let dpi = state.theme.dpi;
    let px = |value| common::scale(value, dpi);
    let disabled = item.itemState & ODS_DISABLED != 0;
    let focused = item.itemState & ODS_FOCUS != 0;
    let pressed = item.itemState & ODS_SELECTED != 0;
    let selected = state.focused_profile == Some(index);
    let dc = item.hDC;
    let rect = item.rcItem;
    unsafe {
        let surface = CreateSolidBrush(if selected {
            0x00F3DFC3
        } else if pressed {
            0x00F5E8D8
        } else {
            0x00FFFFFF
        });
        FillRect(dc, &rect, surface);
        DeleteObject(surface);
        let border = CreateSolidBrush(if focused || selected {
            common::ACCENT
        } else {
            common::BORDER
        });
        FrameRect(dc, &rect, border);
        DeleteObject(border);
        if selected {
            let marker = CreateSolidBrush(common::ACCENT);
            let marker_rect = RECT {
                left: rect.left,
                top: rect.top,
                right: rect.left + px(4),
                bottom: rect.bottom,
            };
            FillRect(dc, &marker_rect, marker);
            DeleteObject(marker);
        }
        let text_left = rect.left + px(56);
        let text_right = rect.right - px(12);
        let text_rect = |y, height| RECT {
            left: text_left,
            top: rect.top + px(y),
            right: text_right,
            bottom: rect.top + px(y + height),
        };
        let color = if disabled {
            common::MUTED
        } else {
            common::TEXT
        };
        common::draw_text(
            dc,
            profile.display_name(),
            text_rect(12, 24),
            state.theme.bold,
            color,
        );
        common::draw_text(
            dc,
            profile.browser_type.display_name(),
            text_rect(38, 22),
            state.theme.font,
            common::ACCENT,
        );
        common::draw_text(
            dc,
            &profile.comment,
            text_rect(62, 22),
            state.theme.font,
            common::MUTED,
        );
        let bitmap = profile
            .icon_path
            .as_ref()
            .and_then(|path| state.loaded_bitmaps.get(path))
            .copied()
            .unwrap_or(null_mut());
        if !bitmap.is_null() {
            let memory = CreateCompatibleDC(dc);
            let previous = SelectObject(memory, bitmap);
            StretchBlt(
                dc,
                rect.left + px(12),
                rect.top + px(16),
                px(32),
                px(32),
                memory,
                0,
                0,
                32,
                32,
                SRCCOPY,
            );
            SelectObject(memory, previous);
            DeleteDC(memory);
        } else {
            let glyph = match profile.browser_type {
                crate::model::BrowserType::Edge => "E",
                crate::model::BrowserType::Chrome => "C",
                crate::model::BrowserType::Firefox => "F",
            };
            common::draw_text(
                dc,
                glyph,
                RECT {
                    left: rect.left + px(18),
                    top: rect.top + px(16),
                    right: rect.left + px(46),
                    bottom: rect.top + px(48),
                },
                state.theme.title,
                common::ACCENT,
            );
        }
        if focused {
            let focus = RECT {
                left: rect.left + px(3),
                top: rect.top + px(3),
                right: rect.right - px(3),
                bottom: rect.bottom - px(3),
            };
            DrawFocusRect(dc, &focus);
        }
    }
}

fn cancel_escape(window: HWND, state: &mut UiState) {
    state.escape_started = None;
    unsafe {
        KillTimer(window, TIMER_HOLD);
    }
    refresh_status(state);
}

fn handle_navigation_message(window: HWND, pointer: *mut UiState, message: &MSG) -> bool {
    let state = unsafe { &mut *pointer };
    if message.message == WM_KEYDOWN && message.wParam == VK_ESCAPE as usize {
        if state.escape_started.is_none() {
            state.escape_started = Some(Instant::now());
            unsafe {
                SetTimer(window, TIMER_HOLD, 50, None);
            }
        }
        return true;
    }
    if message.message == WM_KEYUP && message.wParam == VK_ESCAPE as usize {
        cancel_escape(window, state);
        return true;
    }
    if message.message != WM_KEYDOWN {
        return false;
    }
    let focus = unsafe { GetFocus() };
    let key = message.wParam as u16;
    let current = state
        .profile_buttons
        .iter()
        .position(|button| *button == focus);
    if let Some(current) = current {
        let direction = match key {
            VK_LEFT => Some((-1, 0)),
            VK_RIGHT => Some((1, 0)),
            VK_UP => Some((0, -1)),
            VK_DOWN => Some((0, 1)),
            _ => None,
        };
        if let Some((horizontal, vertical)) = direction {
            let layout = SelectorLayout::new(
                Page::width(window, state.theme.dpi).max(360),
                &state.filtered_indices,
            );
            if let Some(target) = layout.neighbor(current, horizontal, vertical) {
                unsafe {
                    SetFocus(state.profile_buttons[target]);
                }
                state.focused_profile = state.filtered_indices[target];
                refresh_selection_visuals(state);
                state
                    .page
                    .reveal(window, state.profile_buttons[target], state.theme.dpi);
            }
            return true;
        }
    }
    if key == VK_RETURN
        && let Some(current) = current
        && let Some(profile_index) = state.filtered_indices.get(current).copied().flatten()
        && let Some(url) = state.url.clone()
    {
        state.result = SelectorResult::Profile {
            profile_id: state.profiles[profile_index].id,
            url: Some(url),
        };
        unsafe {
            DestroyWindow(window);
        }
        return true;
    }
    if key == VK_RETURN && common::is_button(focus) {
        unsafe {
            SendMessageW(focus, BM_CLICK, 0, 0);
        }
        return true;
    }
    false
}

fn profile_slots(profiles: &[Profile]) -> Vec<Option<usize>> {
    let mut slots = vec![None; MAX_TILES];
    for (slot, (profile_index, _)) in profiles
        .iter()
        .enumerate()
        .filter(|(_, profile)| profile.favorite && profile.available && profile.enabled)
        .take(5)
        .enumerate()
    {
        slots[slot] = Some(profile_index);
    }
    for (offset, (profile_index, _)) in profiles
        .iter()
        .enumerate()
        .filter(|(_, profile)| !profile.favorite && profile.available && profile.enabled)
        .take(20)
        .enumerate()
    {
        slots[5 + offset] = Some(profile_index);
    }
    slots
}

pub fn show_error(title: &str, message: &str) {
    show_error_for(null_mut(), title, message);
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn show_error_for(owner: HWND, title: &str, message: &str) {
    common::release_topmost(owner);
    let title = wide(title);
    let message = wide(message);
    unsafe {
        MessageBoxW(
            owner,
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_APPLMODAL | MB_ICONERROR | MB_SETFOREGROUND,
        );
    }
}

pub fn show_information(title: &str, message: &str) {
    show_information_for(null_mut(), title, message);
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn show_information_for(owner: HWND, title: &str, message: &str) {
    common::release_topmost(owner);
    let title = wide(title);
    let message = wide(message);
    unsafe {
        MessageBoxW(
            owner,
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_APPLMODAL | MB_ICONINFORMATION | MB_SETFOREGROUND,
        );
    }
}

pub fn ask_integrity_recovery(has_valid_backup: bool) -> IntegrityRecoveryAction {
    let title = wide(&app_title::settings_file_error());
    if has_valid_backup {
        let message = wide(
            "設定ファイルが変更または破損しています。\r\n\r\n「はい」: バックアップから復旧\r\n「いいえ」: 破損ファイルを退避して初期化\r\n「キャンセル」: 終了",
        );
        match unsafe {
            MessageBoxW(
                null_mut(),
                message.as_ptr(),
                title.as_ptr(),
                MB_YESNOCANCEL | MB_APPLMODAL | MB_ICONWARNING | MB_SETFOREGROUND,
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
                MB_OKCANCEL | MB_APPLMODAL | MB_ICONWARNING | MB_SETFOREGROUND,
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

    #[test]
    fn keyboard_focus_changes_the_favorite_action_target() {
        let mut state = Box::new(UiState::new(
            None,
            vec![profile(1, true), profile(2, false)],
            10,
            true,
            None,
        ));
        let window =
            common::HiddenTestWindow::new(Some(window_proc), (&mut *state as *mut UiState).cast());
        let tile = state.profile_buttons[5];
        assert_ne!(unsafe { GetWindowLongW(tile, GWL_STYLE) } & BS_NOTIFY, 0);
        unsafe { SendMessageW(tile, WM_SETFOCUS, 0, 0) };
        assert_eq!(state.focused_profile, Some(1));
        assert_ne!(
            unsafe {
                windows_sys::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(state.favorite_up)
            },
            0
        );
        unsafe {
            SendMessageW(
                window.0,
                WM_COMMAND,
                ID_FAVORITE_UP as usize,
                state.favorite_up as isize,
            )
        };
        assert_eq!(
            state.result,
            SelectorResult::FavoriteChanged {
                profile_id: 2,
                favorite: true,
                url: None
            }
        );
    }

    #[test]
    fn click_mode_requires_the_configured_activation_and_keeps_original_url() {
        let url = ValidatedUrl::parse("https://example.com/a?x=%20").unwrap();
        for double_click in [true, false] {
            let mut state = Box::new(UiState::new(
                Some(url.clone()),
                vec![profile(9, false)],
                10,
                double_click,
                None,
            ));
            let window = common::HiddenTestWindow::new(
                Some(window_proc),
                (&mut *state as *mut UiState).cast(),
            );
            unsafe { SendMessageW(window.0, WM_COMMAND, (ID_PROFILE_BASE + 5) as usize, 0) };
            if double_click {
                assert_eq!(state.result, SelectorResult::Cancelled);
                assert_ne!(unsafe { IsWindow(window.0) }, 0);
                unsafe {
                    SendMessageW(
                        window.0,
                        WM_COMMAND,
                        (ID_PROFILE_BASE + 5) as usize | ((BN_DOUBLECLICKED as usize) << 16),
                        0,
                    )
                };
            }
            assert_eq!(
                state.result,
                SelectorResult::Profile {
                    profile_id: 9,
                    url: Some(url.clone())
                }
            );
        }
    }

    #[test]
    fn selection_survives_favorite_reordering_and_skips_unavailable_profiles() {
        let mut hidden = profile(1, true);
        hidden.enabled = false;
        let profiles = vec![hidden, profile(2, true), profile(3, false)];
        let state = UiState::new(None, profiles, 10, true, Some(3));
        assert_eq!(state.focused_profile, Some(2));
        assert_eq!(state.filtered_indices[0], Some(1));
        assert_eq!(state.filtered_indices[5], Some(2));
    }

    #[test]
    fn refresh_button_preserves_url_and_selected_profile() {
        let url = ValidatedUrl::parse("https://example.com/keep?value=1").unwrap();
        let mut state = Box::new(UiState::new(
            Some(url.clone()),
            vec![profile(7, false)],
            10,
            true,
            Some(7),
        ));
        let window =
            common::HiddenTestWindow::new(Some(window_proc), (&mut *state as *mut UiState).cast());
        unsafe {
            SendMessageW(
                window.0,
                WM_COMMAND,
                ID_REFRESH as usize,
                state.refresh as isize,
            )
        };
        assert_eq!(
            state.result,
            SelectorResult::RefreshRequested {
                url: Some(url),
                selected_profile_id: Some(7),
            }
        );
    }

    #[test]
    fn completed_background_scan_returns_changes_without_losing_selection() {
        let url = ValidatedUrl::parse("https://example.com/background").unwrap();
        let detected = DetectedProfile {
            browser_type: BrowserType::Edge,
            profile_id: "Profile 7".into(),
            detected_name: "変更後".into(),
            icon_path: None,
        };
        let mut state = Box::new(UiState::new(
            Some(url.clone()),
            vec![profile(7, false)],
            10,
            true,
            Some(7),
        ));
        state.background_scan = Some(BackgroundProfileScan::completed(vec![detected.clone()]));
        let window =
            common::HiddenTestWindow::new(Some(window_proc), (&mut *state as *mut UiState).cast());
        unsafe { SendMessageW(window.0, WM_TIMER, TIMER_PROFILE_SCAN, 0) };
        assert_eq!(
            state.result,
            SelectorResult::ProfilesDetected {
                detected: vec![detected],
                url: Some(url),
                selected_profile_id: Some(7),
            }
        );
    }

    #[test]
    fn deferred_url_returns_to_rule_evaluation_and_hidden_favorites_count_toward_limit() {
        let profiles = (0..6)
            .map(|id| {
                let mut profile = profile(id, id < 5);
                profile.available = id == 5;
                profile
            })
            .collect();
        let incoming = ValidatedUrl::parse("https://example.net/new?x=%20").unwrap();
        let mut state = Box::new(UiState::new(
            Some(incoming.clone()),
            profiles,
            10,
            true,
            None,
        ));
        let window =
            common::HiddenTestWindow::new(Some(window_proc), (&mut *state as *mut UiState).cast());
        assert_eq!(
            unsafe {
                windows_sys::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(state.favorite_up)
            },
            0
        );
        unsafe { SendMessageW(window.0, WM_URL_RECEIVED, 0, 0) };
        assert_eq!(state.result, SelectorResult::UrlReceived(incoming));
    }

    #[test]
    fn releasing_escape_cancels_the_timer_before_the_next_hold() {
        let mut state = Box::new(UiState::new(None, vec![], 1, true, None));
        let window =
            common::HiddenTestWindow::new(Some(window_proc), (&mut *state as *mut UiState).cast());
        let mut message: MSG = unsafe { std::mem::zeroed() };
        message.message = WM_KEYDOWN;
        message.wParam = VK_ESCAPE as usize;
        handle_navigation_message(window.0, &mut *state, &message);
        assert!(state.escape_started.is_some());
        message.message = WM_KEYUP;
        handle_navigation_message(window.0, &mut *state, &message);
        unsafe { SendMessageW(window.0, WM_TIMER, TIMER_HOLD, 0) };
        assert_ne!(unsafe { IsWindow(window.0) }, 0);
        state.escape_started = Some(Instant::now() - Duration::from_millis(150));
        unsafe { SendMessageW(window.0, WM_TIMER, TIMER_HOLD, 0) };
        assert_eq!(unsafe { IsWindow(window.0) }, 0);
    }
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
        let slots = profile_slots(&profiles);
        assert_eq!(slots[0], Some(0));
        assert_eq!(slots[1], None);
        assert_eq!(slots[5], Some(1));
        assert_eq!(slots[6], Some(2));
    }
}
