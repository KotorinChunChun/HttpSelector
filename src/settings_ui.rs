use crate::app_title;
use crate::model::Profile;
use crate::storage::{MAX_ESCAPE_HOLD_DURATION_TENTHS, MIN_ESCAPE_HOLD_DURATION_TENTHS};
use crate::ui::show_error_for;
use crate::ui_layout::SettingsLayout;
use crate::ui_support::{self as common, Page, Theme, wide, window_text};
use anyhow::ensure;
use std::mem::size_of;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{BST_CHECKED, BST_UNCHECKED, DRAWITEMSTRUCT};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetFocus, SetFocus, VK_ESCAPE, VK_RETURN,
};
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
const ID_ESCAPE_HOLD_DURATION: i32 = 111;
const ID_OPEN_PROFILE_ON_DOUBLE_CLICK: i32 = 112;
const ID_RULE_LIST: i32 = 113;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsAction {
    Saved,
    Rescan,
    RuleList,
    Cancelled,
}

pub struct SettingsOutcome {
    pub action: SettingsAction,
    pub profiles: Vec<Profile>,
    pub pending_url: Option<crate::url_input::ValidatedUrl>,
    pub record_url_history: bool,
    pub escape_hold_duration_tenths: u32,
    pub open_profile_on_double_click: bool,
}

struct SettingsState {
    profiles: Vec<Profile>,
    theme: Theme,
    page: Page,
    fixed: Vec<HWND>,
    list: HWND,
    list_heading: HWND,
    identity: HWND,
    alias: HWND,
    comment: HWND,
    favorite: HWND,
    enabled: HWND,
    up: HWND,
    down: HWND,
    record_url_history: HWND,
    escape_hold_duration: HWND,
    open_profile_on_double_click: HWND,
    rule_list: HWND,
    record_url_history_enabled: bool,
    escape_hold_duration_tenths: u32,
    open_profile_on_double_click_enabled: bool,
    selected: Option<usize>,
    action: SettingsAction,
    pending_url: Option<crate::url_input::ValidatedUrl>,
}

impl SettingsState {
    fn new(
        mut profiles: Vec<Profile>,
        record_url_history_enabled: bool,
        escape_hold_duration_tenths: u32,
        open_profile_on_double_click_enabled: bool,
    ) -> Self {
        profiles.sort_by_key(|profile| (profile.sort_order, profile.id));
        let escape_hold_duration_tenths = escape_hold_duration_tenths.clamp(
            MIN_ESCAPE_HOLD_DURATION_TENTHS,
            MAX_ESCAPE_HOLD_DURATION_TENTHS,
        );
        Self {
            profiles,
            theme: Theme::new(96),
            page: Page::default(),
            fixed: Vec::new(),
            list: null_mut(),
            list_heading: null_mut(),
            identity: null_mut(),
            alias: null_mut(),
            comment: null_mut(),
            favorite: null_mut(),
            enabled: null_mut(),
            up: null_mut(),
            down: null_mut(),
            record_url_history: null_mut(),
            escape_hold_duration: null_mut(),
            open_profile_on_double_click: null_mut(),
            rule_list: null_mut(),
            record_url_history_enabled,
            escape_hold_duration_tenths,
            open_profile_on_double_click_enabled,
            selected: None,
            action: SettingsAction::Cancelled,
            pending_url: None,
        }
    }
}

pub fn run_settings(
    profiles: Vec<Profile>,
    record_url_history_enabled: bool,
    escape_hold_duration_tenths: u32,
    open_profile_on_double_click_enabled: bool,
) -> anyhow::Result<SettingsOutcome> {
    let style =
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | WS_CLIPCHILDREN | WS_VSCROLL;
    let ex_style = WS_EX_CONTROLPARENT;
    let bounds = common::initial_bounds(940, 800, style, ex_style);
    register_window_class()?;
    let pointer = Box::into_raw(Box::new(SettingsState::new(
        profiles,
        record_url_history_enabled,
        escape_hold_duration_tenths,
        open_profile_on_double_click_enabled,
    )));
    let window = unsafe {
        CreateWindowExW(
            ex_style,
            wide(&crate::app_identity::AppIdentity::current()?.window_class(WINDOW_CLASS)).as_ptr(),
            wide(&app_title::settings()).as_ptr(),
            style,
            bounds.x,
            bounds.y,
            bounds.width,
            bounds.height,
            null_mut(),
            null_mut(),
            GetModuleHandleW(null()),
            pointer.cast(),
        )
    };
    if window.is_null() {
        unsafe {
            drop(Box::from_raw(pointer));
        }
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
        if common::handle_edit_shortcut(&message) {
            continue;
        }
        if message.message == WM_KEYDOWN {
            let focus = unsafe { GetFocus() };
            if message.wParam == VK_ESCAPE as usize {
                unsafe {
                    SendMessageW(window, WM_CLOSE, 0, 0);
                }
                continue;
            }
            if message.wParam == VK_RETURN as usize && common::is_button(focus) {
                unsafe {
                    SendMessageW(focus, BM_CLICK, 0, 0);
                }
                continue;
            }
        }
        if unsafe { IsDialogMessageW(window, &message) } == 0 {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        if message.message == WM_KEYDOWN && unsafe { IsWindow(window) } != 0 {
            let state = unsafe { &mut *pointer };
            state
                .page
                .reveal(window, unsafe { GetFocus() }, state.theme.dpi);
        }
    }
    let state = unsafe { Box::from_raw(pointer) };
    Ok(SettingsOutcome {
        action: state.action,
        profiles: state.profiles,
        pending_url: state.pending_url,
        record_url_history: state.record_url_history_enabled,
        escape_hold_duration_tenths: state.escape_hold_duration_tenths,
        open_profile_on_double_click: state.open_profile_on_double_click_enabled,
    })
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
        unsafe {
            SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
        }
    }
    let pointer = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) as *mut SettingsState };
    match message {
        WM_SYSCOMMAND if common::is_maximize_command(wparam) => 0,
        WM_CREATE if !pointer.is_null() => {
            let state = unsafe { &mut *pointer };
            state.theme = Theme::new(unsafe { GetDpiForWindow(window) }.max(96));
            create_controls(window, state);
            0
        }
        WM_SIZE if !pointer.is_null() => {
            let state = unsafe { &mut *pointer };
            if !state.list.is_null() {
                layout_controls(window, state);
            }
            0
        }
        WM_DPICHANGED if !pointer.is_null() => {
            let state = unsafe { &mut *pointer };
            let theme = Theme::new((wparam & 0xffff) as u32);
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
            update_list_extent(state);
            0
        }
        WM_GETMINMAXINFO => {
            let info = unsafe { &mut *(lparam as *mut MINMAXINFO) };
            let dpi = unsafe { GetDpiForWindow(window) }.max(96);
            info.ptMinTrackSize.x = common::scale(400, dpi).min(info.ptMaxTrackSize.x);
            info.ptMinTrackSize.y = common::scale(320, dpi).min(info.ptMaxTrackSize.y);
            0
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
            let field = lparam as HWND == state.identity || message == WM_CTLCOLOREDIT;
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
            common::draw_button(item);
            1
        }
        WM_COMMAND if !pointer.is_null() => {
            let state = unsafe { &mut *pointer };
            let id = (wparam & 0xffff) as i32;
            let notification = ((wparam >> 16) & 0xffff) as u16;
            if id == ID_LIST && notification == LBN_SELCHANGE as u16 {
                let selected = unsafe { SendMessageW(state.list, LB_GETCURSEL, 0, 0) };
                save_current_editor(state);
                if selected != LB_ERR as isize {
                    refresh_list(state, Some(selected as usize));
                }
            } else if (id == ID_UP || id == ID_DOWN) && notification == BN_CLICKED as u16 {
                move_selected(state, if id == ID_UP { -1 } else { 1 });
            } else if (id == ID_SAVE || id == ID_RESCAN) && notification == BN_CLICKED as u16 {
                save_current_editor(state);
                if !capture_general_settings(window, state) {
                    return 0;
                }
                normalize_sort_order(&mut state.profiles);
                state.action = if id == ID_SAVE {
                    SettingsAction::Saved
                } else {
                    SettingsAction::Rescan
                };
                unsafe {
                    DestroyWindow(window);
                }
            } else if id == ID_RULE_LIST && notification == BN_CLICKED as u16 {
                save_current_editor(state);
                if capture_general_settings(window, state) {
                    normalize_sort_order(&mut state.profiles);
                    state.action = SettingsAction::RuleList;
                    unsafe {
                        DestroyWindow(window);
                    }
                }
            } else if id == ID_CANCEL && notification == BN_CLICKED as u16 {
                state.action = SettingsAction::Cancelled;
                unsafe {
                    DestroyWindow(window);
                }
            }
            0
        }
        WM_CLOSE => {
            if !pointer.is_null() {
                unsafe {
                    (*pointer).action = SettingsAction::Cancelled;
                }
            }
            unsafe {
                DestroyWindow(window);
            }
            0
        }
        WM_COPYDATA => {
            if pointer.is_null() {
                return 0;
            }
            let Some(url) = (unsafe { crate::single_instance::received_url(lparam) }) else {
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
            unsafe {
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

fn create_controls(window: HWND, state: &mut SettingsState) {
    let font = state.theme.font;
    let title = common::label(window, "プロファイル設定", state.theme.title);
    let subtitle = common::label(
        window,
        "一覧から選んで編集します。変更は「保存」で反映されます。",
        font,
    );
    state.list_heading = common::label(window, "", state.theme.bold);
    state.list = common::control(
        window,
        "LISTBOX",
        "",
        ID_LIST,
        WS_TABSTOP
            | WS_BORDER
            | WS_VSCROLL
            | WS_HSCROLL
            | LBS_NOTIFY as u32
            | LBS_NOINTEGRALHEIGHT as u32,
        font,
    );
    state.up = common::button(window, "上へ", ID_UP, font);
    state.down = common::button(window, "下へ", ID_DOWN, font);
    let identity_label = common::label(window, "選択中のプロファイル", state.theme.bold);
    state.identity = common::edit(window, 0, true, true, font);
    let alias_label = common::label(window, "表示名", state.theme.bold);
    state.alias = common::edit(window, ID_ALIAS, false, false, font);
    common::cue(state.alias, "空欄の場合は検出名を使用");
    let alias_help = common::label(window, "ブラウザ側のプロファイル名は変更しません。", font);
    let comment_label = common::label(window, "メモ", state.theme.bold);
    state.comment = common::edit(window, ID_COMMENT, true, false, font);
    unsafe {
        SetWindowLongPtrW(
            state.comment,
            GWL_STYLE,
            GetWindowLongPtrW(state.comment, GWL_STYLE) | ES_WANTRETURN as isize,
        );
    }
    state.favorite = common::control(
        window,
        "BUTTON",
        "お気に入り（表示は先頭5件）",
        ID_FAVORITE,
        WS_TABSTOP | BS_AUTOCHECKBOX as u32,
        font,
    );
    state.enabled = common::control(
        window,
        "BUTTON",
        "選択画面に表示する",
        ID_ENABLED,
        WS_TABSTOP | BS_AUTOCHECKBOX as u32,
        font,
    );
    let general = common::label(window, "全体設定", state.theme.bold);
    let hold_duration_label = common::label(window, "ESC長押し終了時間（秒）", state.theme.bold);
    state.escape_hold_duration = common::edit(window, ID_ESCAPE_HOLD_DURATION, false, false, font);
    common::cue(state.escape_hold_duration, "例: 1.0");
    common::set_text(
        state.escape_hold_duration,
        &format_escape_hold_duration(state.escape_hold_duration_tenths),
    );
    let hold_duration_help = common::label(window, "0.1～10.0秒を0.1秒刻みで指定します。", font);
    state.record_url_history = common::control(
        window,
        "BUTTON",
        "URLパターンを記録する（既定OFF）",
        ID_RECORD_URL_HISTORY,
        WS_TABSTOP | BS_AUTOCHECKBOX as u32,
        font,
    );
    set_check(state.record_url_history, state.record_url_history_enabled);
    state.open_profile_on_double_click = common::control(
        window,
        "BUTTON",
        "プロファイルはダブルクリックで開く（OFFでシングルクリック）",
        ID_OPEN_PROFILE_ON_DOUBLE_CLICK,
        WS_TABSTOP | BS_AUTOCHECKBOX as u32,
        font,
    );
    set_check(
        state.open_profile_on_double_click,
        state.open_profile_on_double_click_enabled,
    );
    let history_help = common::label(
        window,
        "生のURLは保存しません。末尾・パラメータ値・フラグメントを * に置き換えたパターンを記録します。\r\n例: /folder/*?tenant=*",
        font,
    );
    let save_help = common::label(
        window,
        "再検出する場合も、編集中の内容を先に保存します。",
        font,
    );
    state.rule_list = common::button(window, "既定ルール一覧", ID_RULE_LIST, font);
    let cancel = common::button(window, "キャンセル", ID_CANCEL, font);
    let save = common::button(window, "保存", ID_SAVE, state.theme.bold);
    let rescan = common::button(window, "保存して再スキャン", ID_RESCAN, font);
    state.fixed = vec![
        title,
        subtitle,
        state.list_heading,
        state.list,
        state.up,
        state.down,
        identity_label,
        state.identity,
        alias_label,
        state.alias,
        alias_help,
        comment_label,
        state.comment,
        state.favorite,
        state.enabled,
        general,
        hold_duration_label,
        state.escape_hold_duration,
        hold_duration_help,
        state.record_url_history,
        state.open_profile_on_double_click,
        history_help,
        save_help,
        cancel,
        save,
        rescan,
        state.rule_list,
    ];
    layout_controls(window, state);
    refresh_list(state, Some(0));
    unsafe {
        SetFocus(state.list);
    }
}

fn layout_controls(window: HWND, state: &mut SettingsState) {
    let layout = SettingsLayout::new(Page::width(window, state.theme.dpi).max(360));
    state.page.controls.clear();
    for (control, bounds) in state.fixed.iter().zip(layout.fixed) {
        state.page.place(*control, bounds);
    }
    state.page.height = layout.height;
    state.page.apply(window, state.theme.dpi);
}

fn save_current_editor(state: &mut SettingsState) {
    let Some(index) = state.selected else {
        return;
    };
    if let Some(profile) = state.profiles.get_mut(index) {
        profile.alias_name = window_text(state.alias);
        profile.comment = window_text(state.comment);
        profile.favorite =
            unsafe { SendMessageW(state.favorite, BM_GETCHECK, 0, 0) } == BST_CHECKED as isize;
        profile.enabled =
            unsafe { SendMessageW(state.enabled, BM_GETCHECK, 0, 0) } == BST_CHECKED as isize;
    }
}

fn capture_general_settings(window: HWND, state: &mut SettingsState) -> bool {
    if state
        .profiles
        .iter()
        .filter(|profile| profile.favorite)
        .count()
        > 5
    {
        show_error_for(
            window,
            &app_title::settings(),
            "お気に入りは5件までです。不要な項目のチェックを外してください。",
        );
        return false;
    }
    let Some(escape_hold_duration_tenths) =
        parse_escape_hold_duration(&window_text(state.escape_hold_duration))
    else {
        show_error_for(
            window,
            &app_title::settings(),
            "ESC長押し終了時間は0.1秒から10.0秒まで、0.1秒刻みで指定してください。",
        );
        unsafe {
            SetFocus(state.escape_hold_duration);
        }
        return false;
    };
    state.escape_hold_duration_tenths = escape_hold_duration_tenths;
    state.record_url_history_enabled =
        unsafe { SendMessageW(state.record_url_history, BM_GETCHECK, 0, 0) }
            == BST_CHECKED as isize;
    state.open_profile_on_double_click_enabled =
        unsafe { SendMessageW(state.open_profile_on_double_click, BM_GETCHECK, 0, 0) }
            == BST_CHECKED as isize;
    true
}

fn set_check(window: HWND, checked: bool) {
    unsafe {
        SendMessageW(
            window,
            BM_SETCHECK,
            if checked { BST_CHECKED } else { BST_UNCHECKED } as usize,
            0,
        );
    }
}

fn load_editor(state: &SettingsState) {
    let profile = state.selected.and_then(|index| state.profiles.get(index));
    for control in [state.alias, state.comment, state.favorite, state.enabled] {
        unsafe {
            EnableWindow(control, i32::from(profile.is_some()));
        }
    }
    unsafe {
        EnableWindow(
            state.up,
            i32::from(state.selected.is_some_and(|index| index > 0)),
        );
        EnableWindow(
            state.down,
            i32::from(
                state
                    .selected
                    .is_some_and(|index| index + 1 < state.profiles.len()),
            ),
        );
    }
    if let Some(profile) = profile {
        common::set_text(
            state.identity,
            &format!(
                "{} / {}{}",
                profile.browser_type.display_name(),
                profile.detected_name,
                if profile.available {
                    ""
                } else {
                    "（未検出）"
                }
            ),
        );
        common::set_text(state.alias, &profile.alias_name);
        common::set_text(state.comment, &profile.comment);
        set_check(state.favorite, profile.favorite);
        set_check(state.enabled, profile.enabled);
    } else {
        common::set_text(
            state.identity,
            "プロファイルがありません。再スキャンで検出できます。",
        );
        common::set_text(state.alias, "");
        common::set_text(state.comment, "");
        set_check(state.favorite, false);
        set_check(state.enabled, false);
    }
}

fn move_selected(state: &mut SettingsState, direction: isize) {
    save_current_editor(state);
    let Some(current) = state.selected else {
        return;
    };
    let target = current as isize + direction;
    if target < 0 || target >= state.profiles.len() as isize {
        return;
    }
    state.profiles.swap(current, target as usize);
    normalize_sort_order(&mut state.profiles);
    refresh_list(state, Some(target as usize));
}

fn list_text(profile: &Profile) -> String {
    format!(
        "{}{} | {}{}{}",
        if profile.favorite { "★ " } else { "" },
        profile
            .display_name()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
        profile.browser_type.display_name(),
        if profile.enabled { "" } else { " [非表示]" },
        if profile.available {
            ""
        } else {
            " [未検出]"
        }
    )
}

fn refresh_list(state: &mut SettingsState, selected: Option<usize>) {
    unsafe {
        let top = SendMessageW(state.list, LB_GETTOPINDEX, 0, 0).max(0);
        SendMessageW(state.list, WM_SETREDRAW, 0, 0);
        SendMessageW(state.list, LB_RESETCONTENT, 0, 0);
        for profile in &state.profiles {
            SendMessageW(
                state.list,
                LB_ADDSTRING,
                0,
                wide(&list_text(profile)).as_ptr() as isize,
            );
        }
        state.selected = selected.filter(|index| *index < state.profiles.len());
        SendMessageW(state.list, LB_SETTOPINDEX, top as usize, 0);
        if let Some(index) = state.selected {
            SendMessageW(state.list, LB_SETCURSEL, index, 0);
        }
        SendMessageW(state.list, WM_SETREDRAW, 1, 0);
        InvalidateRect(state.list, null(), 1);
    }
    common::set_text(
        state.list_heading,
        &format!("プロファイル一覧  {}件", state.profiles.len()),
    );
    update_list_extent(state);
    load_editor(state);
}

fn update_list_extent(state: &SettingsState) {
    unsafe {
        let dc = GetDC(state.list);
        let previous = SelectObject(dc, state.theme.font);
        let mut extent = 0;
        for profile in &state.profiles {
            let text = wide(&list_text(profile));
            let mut size = SIZE { cx: 0, cy: 0 };
            GetTextExtentPoint32W(dc, text.as_ptr(), (text.len() - 1) as i32, &mut size);
            extent = extent.max(size.cx + common::scale(20, state.theme.dpi));
        }
        SelectObject(dc, previous);
        ReleaseDC(state.list, dc);
        SendMessageW(state.list, LB_SETHORIZONTALEXTENT, extent as usize, 0);
    }
}

fn normalize_sort_order(profiles: &mut [Profile]) {
    for (index, profile) in profiles.iter_mut().enumerate() {
        profile.sort_order = index as i64;
    }
}

fn format_escape_hold_duration(tenths: u32) -> String {
    format!("{}.{:01}", tenths / 10, tenths % 10)
}

fn parse_escape_hold_duration(value: &str) -> Option<u32> {
    let value = value.trim();
    let (seconds, fraction) = value.split_once('.').unwrap_or((value, "0"));
    if seconds.is_empty() || fraction.len() != 1 {
        return None;
    }
    let seconds = seconds.parse::<u32>().ok()?;
    let fraction = fraction.parse::<u32>().ok()?;
    let tenths = seconds.checked_mul(10)?.checked_add(fraction)?;
    (MIN_ESCAPE_HOLD_DURATION_TENTHS..=MAX_ESCAPE_HOLD_DURATION_TENTHS)
        .contains(&tenths)
        .then_some(tenths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_hold_duration_uses_one_decimal_place() {
        assert_eq!(parse_escape_hold_duration("1.0"), Some(10));
        assert_eq!(parse_escape_hold_duration("0.1"), Some(1));
        assert_eq!(parse_escape_hold_duration("10"), Some(100));
        assert_eq!(parse_escape_hold_duration("1.05"), None);
        assert_eq!(parse_escape_hold_duration("0.0"), None);
        assert_eq!(parse_escape_hold_duration("10.1"), None);
        assert_eq!(format_escape_hold_duration(10), "1.0");
    }
}
