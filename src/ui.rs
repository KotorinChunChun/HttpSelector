use crate::model::Profile;
use crate::ui_layout::SelectorLayout;
use crate::ui_support::{self as common, Bounds, Page, Theme};
use crate::url_input::ValidatedUrl;
use anyhow::ensure;
use std::collections::HashMap;
use std::mem::size_of;
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{DRAWITEMSTRUCT, ODS_DISABLED, ODS_FOCUS, ODS_SELECTED};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetFocus, SetFocus, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_RETURN, VK_RIGHT, VK_UP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const WINDOW_CLASS: &str = "HttpSelector.MainWindow";
const MAX_TILES: usize = 25;
const HOLD_DURATION: Duration = Duration::from_secs(3);
const TIMER_HOLD: usize = 1;
const ID_SEARCH: i32 = 100;
const ID_SETTINGS: i32 = 101;
const ID_CLOSE: i32 = 102;
const ID_PROFILE_BASE: i32 = 1000;

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
    loaded_bitmaps: HashMap<String, HBITMAP>,
    theme: Theme,
    page: Page,
    fixed: Vec<HWND>,
    favorites_heading: HWND,
    normal_heading: HWND,
    favorite_empty: HWND,
    normal_empty: HWND,
    status: HWND,
    search: HWND,
    original_edit: HWND,
    preview_edit: HWND,
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
            loaded_bitmaps: HashMap::new(),
            theme: Theme::new(96),
            page: Page::default(),
            fixed: Vec::new(),
            favorites_heading: null_mut(),
            normal_heading: null_mut(),
            favorite_empty: null_mut(),
            normal_empty: null_mut(),
            status: null_mut(),
            search: null_mut(),
            original_edit: null_mut(),
            preview_edit: null_mut(),
            escape_started: None,
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
) -> anyhow::Result<SelectorResult> {
    let style = WS_POPUP | WS_THICKFRAME | WS_CLIPCHILDREN | WS_VSCROLL;
    let ex_style = WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_CONTROLPARENT;
    let bounds = common::initial_bounds(1120, 900, style, ex_style);
    register_window_class()?;
    let state_pointer = Box::into_raw(Box::new(UiState::new(url, profiles)));
    let window = unsafe {
        CreateWindowExW(
            ex_style,
            wide(WINDOW_CLASS).as_ptr(),
            wide("HttpSelector").as_ptr(),
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
    let class = wide(WINDOW_CLASS);
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
        WM_CREATE if !pointer.is_null() => {
            let state = unsafe { &mut *pointer };
            state.theme = Theme::new(unsafe { GetDpiForWindow(window) }.max(96));
            create_controls(window, state);
            0
        }
        WM_SIZE if !pointer.is_null() => {
            let state = unsafe { &mut *pointer };
            if !state.search.is_null() {
                layout_controls(window, state);
            }
            0
        }
        WM_DPICHANGED if !pointer.is_null() => {
            let state = unsafe { &mut *pointer };
            let theme = Theme::new(low_word(wparam) as u32);
            common::update_fonts(&state.page, &state.theme, &theme);
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
            draw_tile(state, item);
            1
        }
        WM_COMMAND if !pointer.is_null() => {
            let state = unsafe { &mut *pointer };
            let id = low_word(wparam) as i32;
            let notification = high_word(wparam);
            if id == ID_SEARCH && notification == EN_CHANGE as u16 {
                state.filtered_indices =
                    filtered_slots(&state.profiles, &common::window_text(state.search));
                refresh_tiles(window, state);
            } else if id == ID_SETTINGS && notification == BN_CLICKED as u16 {
                state.result = SelectorResult::Settings {
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
            } else if notification == BN_CLICKED as u16
                && (ID_PROFILE_BASE..ID_PROFILE_BASE + MAX_TILES as i32).contains(&id)
                && state.url.is_some()
            {
                if let Some(index) = state.filtered_indices[(id - ID_PROFILE_BASE) as usize] {
                    state.result = SelectorResult::Profile {
                        profile_id: state.profiles[index].id,
                        url: state.url.clone(),
                    };
                    unsafe {
                        DestroyWindow(window);
                    }
                }
            }
            0
        }
        WM_TIMER if wparam == TIMER_HOLD && !pointer.is_null() => {
            let state = unsafe { &mut *pointer };
            if let Some(started) = state.escape_started {
                let elapsed = started.elapsed();
                if elapsed >= HOLD_DURATION {
                    state.result = SelectorResult::Cancelled;
                    unsafe {
                        DestroyWindow(window);
                    }
                } else {
                    common::set_text(
                        state.status,
                        &format!(
                            "終了まであと {:.1} 秒 — ESCを離すと取り消し",
                            (HOLD_DURATION - elapsed).as_secs_f32()
                        ),
                    );
                }
            }
            0
        }
        WM_COPYDATA if !pointer.is_null() => {
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
            let state = unsafe { &mut *pointer };
            state.url = Some(url);
            refresh_url(window, state);
            unsafe {
                ShowWindow(window, SW_RESTORE);
                SetForegroundWindow(window);
            }
            1
        }
        WM_ACTIVATE if !pointer.is_null() => {
            let state = unsafe { &mut *pointer };
            if low_word(wparam) == WA_INACTIVE as u16 {
                cancel_escape(window, state);
            } else {
                unsafe {
                    SetWindowPos(window, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
                }
            }
            0
        }
        WM_CLOSE => 0,
        WM_DESTROY => {
            unsafe {
                KillTimer(window, TIMER_HOLD);
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

fn create_controls(window: HWND, state: &mut UiState) {
    let font = state.theme.font;
    let title = common::label(window, "HttpSelector", state.theme.title);
    let subtitle = common::label(window, "リンクを開くプロファイルを選択", font);
    let close = common::button(window, "閉じる", ID_CLOSE, font);
    state.status = common::label(window, "", font);
    let original_label = common::label(window, "受信したURL", state.theme.bold);
    state.original_edit = common::edit(window, 0, true, true, font);
    let preview_label = common::label(window, "デコード表示（確認専用）", state.theme.bold);
    state.preview_edit = common::edit(window, 0, true, true, font);
    let search_label = common::label(window, "検索", state.theme.bold);
    state.search = common::edit(window, ID_SEARCH, false, false, font);
    common::cue(state.search, "名前・メモ・ブラウザ名で検索");
    let settings = common::button(window, "設定", ID_SETTINGS, font);
    state.fixed = vec![
        title,
        subtitle,
        close,
        state.status,
        original_label,
        state.original_edit,
        preview_label,
        state.preview_edit,
        search_label,
        state.search,
        settings,
    ];
    state.favorites_heading = common::label(window, "", state.theme.bold);
    state.normal_heading = common::label(window, "", state.theme.bold);
    state.favorite_empty = common::label(window, "", font);
    state.normal_empty = common::label(window, "", font);
    for index in 0..MAX_TILES {
        state.profile_buttons.push(common::control(
            window,
            "BUTTON",
            "",
            ID_PROFILE_BASE + index as i32,
            WS_TABSTOP | BS_OWNERDRAW as u32,
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
    unsafe {
        SetFocus(state.search);
    }
}

fn layout_controls(window: HWND, state: &mut UiState) {
    let width = Page::width(window, state.theme.dpi).max(360);
    let content = width - 48;
    state.page.controls.clear();
    let bounds = [
        Bounds::new(24, 16, content - 110, 32),
        Bounds::new(24, 54, content, 22),
        Bounds::new(width - 120, 18, 96, 32),
        Bounds::new(24, 82, content, 32),
        Bounds::new(24, 118, content, 22),
        Bounds::new(24, 144, content, 42),
        Bounds::new(24, 198, content, 22),
        Bounds::new(24, 224, content, 42),
        Bounds::new(24, 284, 44, 24),
        Bounds::new(76, 280, content - 160, 32),
        Bounds::new(width - 120, 280, 96, 32),
    ];
    for (control, rect) in state.fixed.iter().zip(bounds) {
        state.page.place(*control, rect);
    }
    let layout = SelectorLayout::new(width, &state.filtered_indices);
    state
        .page
        .place(state.favorites_heading, layout.favorites_heading);
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
    common::set_text(
        state.status,
        if state.url.is_some() {
            "クリック / Enterで開く · ESCは3秒長押しで終了"
        } else {
            "URL未指定です。設定からプロファイルを管理できます。"
        },
    );
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
    let searching = !common::window_text(state.search).trim().is_empty();
    common::set_text(
        state.favorite_empty,
        if searching {
            "一致するお気に入りはありません。"
        } else {
            "設定でお気に入りにすると、ここに表示されます。"
        },
    );
    common::set_text(
        state.normal_empty,
        if searching {
            "見つかりません。検索条件を変えてください。"
        } else {
            "表示するプロファイルはありません。設定から確認できます。"
        },
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
            EnableWindow(*button, i32::from(visible && state.url.is_some()));
            ShowWindow(*button, if visible { SW_SHOW } else { SW_HIDE });
            InvalidateRect(*button, null(), 1);
        }
    }
    layout_controls(window, state);
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
    let dc = item.hDC;
    let rect = item.rcItem;
    unsafe {
        let surface = CreateSolidBrush(if pressed { 0x00F5E8D8 } else { 0x00FFFFFF });
        FillRect(dc, &rect, surface);
        DeleteObject(surface);
        let border = CreateSolidBrush(if focused {
            common::ACCENT
        } else {
            common::BORDER
        });
        FrameRect(dc, &rect, border);
        DeleteObject(border);
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
            text_rect(38, 20),
            state.theme.font,
            common::ACCENT,
        );
        common::draw_text(
            dc,
            &profile.comment,
            text_rect(62, 18),
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
    if focus == state.search && (key == VK_DOWN || key == VK_RETURN) && state.url.is_some() {
        if let Some(index) = state.filtered_indices.iter().position(Option::is_some) {
            // 検索欄のEnterはまず候補へ移動し、誤起動を防ぐ。
            unsafe {
                SetFocus(state.profile_buttons[index]);
            }
            state
                .page
                .reveal(window, state.profile_buttons[index], state.theme.dpi);
        }
        return true;
    }
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
                state
                    .page
                    .reveal(window, state.profile_buttons[target], state.theme.dpi);
            }
            return true;
        }
    }
    if key == VK_RETURN && common::is_button(focus) {
        unsafe {
            SendMessageW(focus, BM_CLICK, 0, 0);
        }
        return true;
    }
    false
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
