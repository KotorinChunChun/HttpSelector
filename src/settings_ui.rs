use crate::app_title;
use crate::model::{MAX_FAVORITE_PROFILES, Profile};
use crate::storage::{MAX_ESCAPE_HOLD_DURATION_TENTHS, MIN_ESCAPE_HOLD_DURATION_TENTHS};
use crate::ui_layout::SettingsLayout;
use crate::ui_support::{self as common, Page, Theme, wide, window_text};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{BST_CHECKED, BST_UNCHECKED, DRAWITEMSTRUCT};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, GetFocus, SetFocus};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const WINDOW_CLASS: &str = "HttpSelector.SettingsWindow";
const ID_LIST: i32 = 100;
const ID_ALIAS: i32 = 101;
const ID_COMMENT: i32 = 102;
const ID_FAVORITE: i32 = 103;
const ID_ENABLED: i32 = 104;
const ID_UP: i32 = 105;
const ID_DOWN: i32 = 106;
const ID_TEST_PROFILE: i32 = 107;
const ID_RESCAN: i32 = 108;
const ID_CLOSE: i32 = 109;
const ID_RECORD_URL_HISTORY: i32 = 110;
const ID_ESCAPE_HOLD_DURATION: i32 = 111;
const ID_OPEN_PROFILE_ON_DOUBLE_CLICK: i32 = 112;
const ID_CLEAR_APPROVALS: i32 = 114;
const ID_QUAD9: i32 = 115;
const TIMER_AUTOSAVE: usize = 51;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsAction {
    Closed,
    Rescan,
    ClearDomainApprovals,
}

pub struct SettingsOutcome {
    pub action: SettingsAction,
    pub profiles: Vec<Profile>,
    pub pending_url: Option<crate::url_input::ValidatedUrl>,
    pub record_url_history: bool,
    pub escape_hold_duration_tenths: u32,
    pub open_profile_on_double_click: bool,
    pub quad9_enabled: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsPreferences {
    pub profiles: Vec<Profile>,
    pub record_url_history: bool,
    pub escape_hold_duration_tenths: u32,
    pub open_profile_on_double_click: bool,
    pub quad9_enabled: bool,
}

type SaveCallback<'a> = dyn FnMut(&SettingsPreferences) -> anyhow::Result<()> + 'a;
type TestCallback<'a> = dyn FnMut(&Profile) -> anyhow::Result<()> + 'a;

struct SettingsState<'a> {
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
    clear_approvals: HWND,
    quad9: HWND,
    quad9_enabled: bool,
    test_profile: HWND,
    status: HWND,
    tips: Option<common::Tooltips>,
    loading: bool,
    dirty: bool,
    persisted: SettingsPreferences,
    on_change: Option<&'a mut SaveCallback<'a>>,
    on_test: Option<&'a mut TestCallback<'a>>,
    record_url_history_enabled: bool,
    escape_hold_duration_tenths: u32,
    open_profile_on_double_click_enabled: bool,
    selected: Option<usize>,
    action: SettingsAction,
    pending_url: Option<crate::url_input::ValidatedUrl>,
}

impl<'a> SettingsState<'a> {
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
        let persisted = SettingsPreferences {
            profiles: profiles.clone(),
            record_url_history: record_url_history_enabled,
            escape_hold_duration_tenths,
            open_profile_on_double_click: open_profile_on_double_click_enabled,
            quad9_enabled: false,
        };
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
            clear_approvals: null_mut(),
            quad9: null_mut(),
            quad9_enabled: false,
            test_profile: null_mut(),
            status: null_mut(),
            tips: None,
            loading: true,
            dirty: false,
            persisted,
            on_change: None,
            on_test: None,
            record_url_history_enabled,
            escape_hold_duration_tenths,
            open_profile_on_double_click_enabled,
            selected: None,
            action: SettingsAction::Closed,
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
    run_settings_with_quad9(
        profiles,
        record_url_history_enabled,
        escape_hold_duration_tenths,
        open_profile_on_double_click_enabled,
        false,
    )
}

pub fn run_settings_with_quad9(
    profiles: Vec<Profile>,
    record_url_history_enabled: bool,
    escape_hold_duration_tenths: u32,
    open_profile_on_double_click_enabled: bool,
    quad9_enabled: bool,
) -> anyhow::Result<SettingsOutcome> {
    // UIプレビューは実DB・ブラウザーへ接続しない。
    let mut save = |_: &SettingsPreferences| Ok(());
    let mut test = |_: &Profile| Ok(());
    run_settings_live(
        profiles,
        record_url_history_enabled,
        escape_hold_duration_tenths,
        open_profile_on_double_click_enabled,
        quad9_enabled,
        &mut save,
        &mut test,
    )
}

pub fn run_settings_live<'a>(
    profiles: Vec<Profile>,
    record_url_history_enabled: bool,
    escape_hold_duration_tenths: u32,
    open_profile_on_double_click_enabled: bool,
    quad9_enabled: bool,
    save: &'a mut SaveCallback<'a>,
    test: &'a mut TestCallback<'a>,
) -> anyhow::Result<SettingsOutcome> {
    let style =
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | WS_CLIPCHILDREN | WS_VSCROLL;
    let ex_style = WS_EX_CONTROLPARENT;
    let bounds = common::initial_bounds(940, 620, style, ex_style);
    let class = common::register_window_class(
        WINDOW_CLASS,
        Some(window_proc),
        CS_HREDRAW | CS_VREDRAW,
        "設定画面",
    )?;
    let mut state = Box::new(SettingsState::new(
        profiles,
        record_url_history_enabled,
        escape_hold_duration_tenths,
        open_profile_on_double_click_enabled,
    ));
    state.quad9_enabled = quad9_enabled;
    state.persisted.quad9_enabled = quad9_enabled;
    state.on_change = Some(save);
    state.on_test = Some(test);
    let pointer = Box::into_raw(state);
    let window = unsafe {
        CreateWindowExW(
            ex_style,
            class.as_ptr(),
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
    crate::management::set_active_window(window);
    common::run_message_loop(
        window,
        "設定画面のメッセージを取得できません",
        |message| common::handle_dialog_key(window, message),
        || {
            let state = unsafe { &mut *pointer };
            state
                .page
                .reveal(window, unsafe { GetFocus() }, state.theme.dpi);
        },
    )?;
    let state = unsafe { Box::from_raw(pointer) };
    Ok(SettingsOutcome {
        action: state.action,
        profiles: state.profiles,
        pending_url: state.pending_url,
        record_url_history: state.record_url_history_enabled,
        escape_hold_duration_tenths: state.escape_hold_duration_tenths,
        open_profile_on_double_click: state.open_profile_on_double_click_enabled,
        quad9_enabled: state.quad9_enabled,
    })
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if let Some(result) =
        unsafe { crate::management::handle_window_message(window, message, wparam) }
    {
        return result;
    }
    let pointer = unsafe { common::window_state::<SettingsState<'_>>(window, message, lparam) };
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
            if state.loading {
                return 0;
            }
            let id = (wparam & 0xffff) as i32;
            let notification = ((wparam >> 16) & 0xffff) as u16;
            if id == ID_LIST && notification == LBN_SELCHANGE as u16 {
                let selected = unsafe { SendMessageW(state.list, LB_GETCURSEL, 0, 0) };
                if !persist_settings(window, state) {
                    if let Some(index) = state.selected {
                        unsafe {
                            SendMessageW(state.list, LB_SETCURSEL, index, 0);
                        }
                    }
                    return 0;
                }
                if selected != LB_ERR as isize {
                    refresh_list(state, Some(selected as usize));
                }
            } else if (id == ID_UP || id == ID_DOWN) && notification == BN_CLICKED as u16 {
                if persist_settings(window, state) {
                    move_selected(state, if id == ID_UP { -1 } else { 1 });
                    persist_settings(window, state);
                }
            } else if matches!(id, ID_ALIAS | ID_COMMENT | ID_ESCAPE_HOLD_DURATION) {
                if notification == EN_CHANGE as u16 {
                    state.dirty = true;
                    unsafe {
                        SetTimer(window, TIMER_AUTOSAVE, 500, None);
                    }
                } else if notification == EN_KILLFOCUS as u16 && state.dirty {
                    persist_settings(window, state);
                }
            } else if matches!(
                id,
                ID_FAVORITE
                    | ID_ENABLED
                    | ID_RECORD_URL_HISTORY
                    | ID_OPEN_PROFILE_ON_DOUBLE_CLICK
                    | ID_QUAD9
            ) && notification == BN_CLICKED as u16
            {
                state.dirty = true;
                persist_settings(window, state);
            } else if id == ID_TEST_PROFILE && notification == BN_CLICKED as u16 {
                if persist_settings(window, state) {
                    if let Some(profile) = state
                        .selected
                        .and_then(|index| state.profiles.get(index))
                        .cloned()
                    {
                        let result = state
                            .on_test
                            .as_mut()
                            .map_or(Ok(()), |callback| callback(&profile));
                        common::set_text(
                            state.status,
                            &match result {
                                Ok(()) => "選択したプロファイルを試しに開きました。".to_owned(),
                                Err(error) => format!("試し起動できません: {error:#}"),
                            },
                        );
                    }
                }
            } else if matches!(id, ID_CLOSE | ID_RESCAN | ID_CLEAR_APPROVALS)
                && notification == BN_CLICKED as u16
                && persist_settings(window, state)
            {
                state.action = match id {
                    ID_RESCAN => SettingsAction::Rescan,
                    ID_CLEAR_APPROVALS => SettingsAction::ClearDomainApprovals,
                    _ => SettingsAction::Closed,
                };
                unsafe {
                    DestroyWindow(window);
                }
            }
            0
        }
        WM_TIMER if wparam == TIMER_AUTOSAVE && !pointer.is_null() => {
            persist_settings(window, unsafe { &mut *pointer });
            0
        }
        WM_CLOSE => {
            if !pointer.is_null() {
                let state = unsafe { &mut *pointer };
                if !persist_settings(window, state) {
                    return 0;
                }
                state.action = SettingsAction::Closed;
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
            crate::management::clear_active_window(window);
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

fn create_controls(window: HWND, state: &mut SettingsState<'_>) {
    let font = state.theme.font;
    let title = common::label(window, "プロファイル設定", state.theme.title);
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
    state.test_profile = common::button(window, "試しに開く", ID_TEST_PROFILE, font);
    let identity_label = common::label(window, "選択中のプロファイル", state.theme.bold);
    state.identity = common::edit(window, 0, true, true, font);
    let alias_label = common::label(window, "表示名", state.theme.bold);
    state.alias = common::edit(window, ID_ALIAS, false, false, font);
    common::cue(state.alias, "空欄の場合は検出名を使用");
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
        "お気に入り",
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
    let hold_label = common::label(window, "ESC長押し終了時間（秒）", state.theme.bold);
    state.escape_hold_duration = common::edit(window, ID_ESCAPE_HOLD_DURATION, false, false, font);
    common::set_text(
        state.escape_hold_duration,
        &format_escape_hold_duration(state.escape_hold_duration_tenths),
    );
    state.open_profile_on_double_click = common::control(
        window,
        "BUTTON",
        "ダブルクリックで開く",
        ID_OPEN_PROFILE_ON_DOUBLE_CLICK,
        WS_TABSTOP | BS_AUTOCHECKBOX as u32,
        font,
    );
    state.record_url_history = common::control(
        window,
        "BUTTON",
        "URLパターンを記録する",
        ID_RECORD_URL_HISTORY,
        WS_TABSTOP | BS_AUTOCHECKBOX as u32,
        font,
    );
    state.quad9 = common::control(
        window,
        "BUTTON",
        "Quad9へドメインを照会する（既定OFF）",
        ID_QUAD9,
        WS_TABSTOP | BS_AUTOCHECKBOX as u32,
        font,
    );
    set_check(state.favorite, false);
    set_check(state.enabled, false);
    set_check(state.record_url_history, state.record_url_history_enabled);
    set_check(
        state.open_profile_on_double_click,
        state.open_profile_on_double_click_enabled,
    );
    set_check(state.quad9, state.quad9_enabled);
    let quad9_help = common::label(window, "ONにするとドメインをQuad9へ送信します。", font);
    state.clear_approvals = common::button(
        window,
        "名称警告のドメイン承認をすべて解除",
        ID_CLEAR_APPROVALS,
        font,
    );
    let rescan = common::button(window, "再スキャン", ID_RESCAN, font);
    let close = common::button(window, "閉じる", ID_CLOSE, font);
    state.status = common::label(window, "", font);
    state.fixed = vec![
        title,
        state.list_heading,
        state.list,
        state.up,
        state.down,
        state.test_profile,
        identity_label,
        state.identity,
        alias_label,
        state.alias,
        comment_label,
        state.comment,
        state.favorite,
        state.enabled,
        general,
        hold_label,
        state.escape_hold_duration,
        state.open_profile_on_double_click,
        state.record_url_history,
        state.quad9,
        quad9_help,
        state.clear_approvals,
        rescan,
        close,
        state.status,
    ];
    let mut tips = common::Tooltips::new(window, state.theme.dpi);
    for (control, description) in [
        (
            state.list,
            "編集するプロファイルを選びます。入力を終えると変更を自動保存します。",
        ),
        (
            state.identity,
            "検出したブラウザーとプロファイル名です。ブラウザー側の名前は変更しません。",
        ),
        (
            state.alias,
            "このツール内での表示名です。空欄では検出名を表示します。入力後に自動保存します。",
        ),
        (
            state.comment,
            "プロファイルを見分けるためのメモです。このPCに自動保存します。",
        ),
        (
            state.favorite,
            "先頭5件をお気に入り欄へ表示します。利用可能な先頭がお気に入り既定となり、割り込みOFF時の未登録URLを開きます。",
        ),
        (
            state.enabled,
            "OFFのプロファイルは選択画面と自動振り分けの候補から外れます。",
        ),
        (
            state.up,
            "このプロファイルの表示順を一つ上へ移し、自動保存します。",
        ),
        (
            state.down,
            "このプロファイルの表示順を一つ下へ移し、自動保存します。",
        ),
        (
            state.test_profile,
            "選択したプロファイルで空白ページを開きます。受信URLや外部サイトは開きません。",
        ),
        (
            state.escape_hold_duration,
            "0ではESCを一度押すと終了します。0より大きい場合は指定秒数の長押しで終了します。既定0秒、上限10秒です。",
        ),
        (
            state.open_profile_on_double_click,
            "ONはダブルクリックでプロファイルを開きます。OFFは一回のクリックで開きます。",
        ),
        (
            state.record_url_history,
            "既定OFF。ONでは開いたURLを末尾・パラメーター値・フラグメントを*に置換したパターンとしてこのPCに記録します。完全な匿名化ではありません。",
        ),
        (
            state.quad9,
            "既定OFF。ONにした時点で自動保存し、以後はドメイン名をQuad9へ送信します。パス・パラメーターは送りませんが接続元IPは伝わります。OFFでも端末内の検査は継続します。",
        ),
        (
            quad9_help,
            "送信は任意です。OFFにすると以後の照会を停止します。送信済みの照会は取り消せません。",
        ),
        (
            state.clear_approvals,
            "記憶した名称警告の承認をすべて解除します。振り分けルールやプロファイルは保持します。",
        ),
        (
            rescan,
            "変更を保存した上で、追加・削除されたブラウザープロファイルを再検出します。",
        ),
        (
            close,
            "変更を自動保存して閉じます。保存に失敗した場合は閉じずに理由を表示します。",
        ),
    ] {
        tips.add(window, control, description);
    }
    state.tips = Some(tips);
    layout_controls(window, state);
    refresh_list(state, Some(0));
    state.loading = false;
    unsafe {
        SetFocus(state.list);
    }
}

fn layout_controls(window: HWND, state: &mut SettingsState<'_>) {
    let layout = SettingsLayout::new(Page::width(window, state.theme.dpi).max(360));
    state.page.controls.clear();
    for (control, bounds) in state.fixed.iter().zip(layout.fixed) {
        state.page.place(*control, bounds);
    }
    state.page.height = layout.height;
    state.page.apply(window, state.theme.dpi);
}

fn save_current_editor(state: &mut SettingsState<'_>) {
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

fn snapshot(state: &SettingsState<'_>) -> SettingsPreferences {
    SettingsPreferences {
        profiles: state.profiles.clone(),
        record_url_history: state.record_url_history_enabled,
        escape_hold_duration_tenths: state.escape_hold_duration_tenths,
        open_profile_on_double_click: state.open_profile_on_double_click_enabled,
        quad9_enabled: state.quad9_enabled,
    }
}

fn persist_settings(window: HWND, state: &mut SettingsState<'_>) -> bool {
    unsafe {
        KillTimer(window, TIMER_AUTOSAVE);
    }
    if state.loading {
        return true;
    }
    save_current_editor(state);
    if state
        .profiles
        .iter()
        .filter(|profile| profile.favorite)
        .count()
        > MAX_FAVORITE_PROFILES
    {
        common::set_text(state.status, "保存できません: お気に入りは5件までです。");
        return false;
    }
    let Some(duration) = parse_escape_hold_duration(&window_text(state.escape_hold_duration))
    else {
        common::set_text(
            state.status,
            "保存できません: ESC時間は0〜10秒を0.1秒刻みで指定してください。",
        );
        return false;
    };
    state.escape_hold_duration_tenths = duration;
    state.quad9_enabled =
        unsafe { SendMessageW(state.quad9, BM_GETCHECK, 0, 0) } == BST_CHECKED as isize;
    state.record_url_history_enabled =
        unsafe { SendMessageW(state.record_url_history, BM_GETCHECK, 0, 0) }
            == BST_CHECKED as isize;
    state.open_profile_on_double_click_enabled =
        unsafe { SendMessageW(state.open_profile_on_double_click, BM_GETCHECK, 0, 0) }
            == BST_CHECKED as isize;
    let current = snapshot(state);
    if current == state.persisted {
        state.dirty = false;
        common::set_text(state.status, "");
        return true;
    }
    let result = state
        .on_change
        .as_mut()
        .map_or(Ok(()), |callback| callback(&current));
    match result {
        Ok(()) => {
            state.persisted = current;
            state.dirty = false;
            common::set_text(state.status, "自動保存しました。");
            if let Some(index) = state.selected {
                let top = unsafe { SendMessageW(state.list, LB_GETTOPINDEX, 0, 0) };
                let text = wide(&list_text(&state.profiles[index]));
                unsafe {
                    SendMessageW(state.list, LB_DELETESTRING, index, 0);
                    SendMessageW(state.list, LB_INSERTSTRING, index, text.as_ptr() as isize);
                    SendMessageW(state.list, LB_SETCURSEL, index, 0);
                    SendMessageW(state.list, LB_SETTOPINDEX, top as usize, 0);
                }
                update_list_extent(state);
            }
            true
        }
        Err(error) => {
            state.dirty = true;
            common::set_text(state.status, &format!("保存できません: {error:#}"));
            false
        }
    }
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

fn load_editor(state: &SettingsState<'_>) {
    let profile = state.selected.and_then(|index| state.profiles.get(index));
    for control in [state.alias, state.comment, state.favorite, state.enabled] {
        unsafe {
            EnableWindow(control, i32::from(profile.is_some()));
        }
    }
    unsafe {
        EnableWindow(
            state.test_profile,
            i32::from(profile.is_some_and(|profile| profile.available)),
        );
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

fn move_selected(state: &mut SettingsState<'_>, direction: isize) {
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

fn refresh_list(state: &mut SettingsState<'_>, selected: Option<usize>) {
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
    let loading = state.loading;
    state.loading = true;
    load_editor(state);
    state.loading = loading;
}

fn update_list_extent(state: &SettingsState<'_>) {
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
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    fn sample_profile() -> Profile {
        Profile {
            id: 1,
            browser_type: crate::model::BrowserType::Chrome,
            profile_id: "Default".into(),
            detected_name: "テスト".into(),
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
    fn quad9_click_is_explicit_and_immediately_saved_and_tooltips_are_real_controls() {
        let saved = Rc::new(RefCell::new(Vec::new()));
        let records = saved.clone();
        let mut save = move |preferences: &SettingsPreferences| {
            records.borrow_mut().push(preferences.clone());
            Ok(())
        };
        let mut state = Box::new(SettingsState::new(vec![], false, 0, true));
        state.on_change = Some(&mut save);
        let window = common::HiddenTestWindow::new(
            Some(window_proc),
            (&mut *state as *mut SettingsState<'_>).cast(),
        );
        assert!(saved.borrow().is_empty());
        assert!(!state.quad9_enabled);
        let tips = state.tips.as_ref().unwrap();
        assert_eq!(tips.count(), 17);
        assert!(
            tips.contains(state.quad9)
                && tips.contains(state.alias)
                && tips.contains(state.escape_hold_duration)
        );
        unsafe {
            SendMessageW(state.quad9, BM_CLICK, 0, 0);
        }
        assert!(state.persisted.quad9_enabled);
        assert_eq!(saved.borrow().len(), 1);
        unsafe {
            SendMessageW(state.quad9, BM_CLICK, 0, 0);
        }
        assert!(!state.persisted.quad9_enabled);
        assert_eq!(saved.borrow().len(), 2);
        assert_ne!(unsafe { IsWindow(window.0) }, 0);
    }

    #[test]
    fn profile_edits_autosave_to_database_test_launch_and_close_flush_the_last_input() {
        let temp = tempfile::tempdir().unwrap();
        let storage = crate::storage::Storage::new(temp.path().join("settings.db"));
        storage.initialize().unwrap();
        storage
            .sync_detected(&[crate::model::DetectedProfile {
                browser_type: crate::model::BrowserType::Chrome,
                profile_id: "Default".into(),
                detected_name: "テスト".into(),
                icon_path: None,
            }])
            .unwrap();
        let mut save = |preferences: &SettingsPreferences| {
            storage.save_preferences_with_quad9(
                &preferences.profiles,
                preferences.record_url_history,
                preferences.escape_hold_duration_tenths,
                preferences.open_profile_on_double_click,
                preferences.quad9_enabled,
            )
        };
        let tested = Rc::new(RefCell::new(Vec::new()));
        let test_records = tested.clone();
        let mut test = move |profile: &Profile| {
            test_records.borrow_mut().push(profile.clone());
            Ok(())
        };
        let mut state = Box::new(SettingsState::new(
            storage.load_all_profiles().unwrap(),
            false,
            0,
            true,
        ));
        state.on_change = Some(&mut save);
        state.on_test = Some(&mut test);
        let window = common::HiddenTestWindow::new(
            Some(window_proc),
            (&mut *state as *mut SettingsState<'_>).cast(),
        );
        common::set_text(state.alias, "仕事用・変更");
        unsafe {
            SendMessageW(window.0, WM_TIMER, TIMER_AUTOSAVE, 0);
        }
        assert_eq!(
            storage.load_all_profiles().unwrap()[0].alias_name,
            "仕事用・変更"
        );
        unsafe {
            SendMessageW(state.test_profile, BM_CLICK, 0, 0);
        }
        assert_eq!(tested.borrow()[0].alias_name, "仕事用・変更");
        common::set_text(state.comment, "閉じる直前のメモ");
        unsafe {
            SendMessageW(window.0, WM_CLOSE, 0, 0);
        }
        assert_eq!(
            storage.load_all_profiles().unwrap()[0].comment,
            "閉じる直前のメモ"
        );
        assert_eq!(unsafe { IsWindow(window.0) }, 0);
    }

    #[test]
    fn failed_autosave_and_invalid_duration_keep_the_window_and_old_committed_state() {
        let fail = Rc::new(Cell::new(true));
        let callback_fail = fail.clone();
        let mut save = move |_: &SettingsPreferences| {
            anyhow::ensure!(!callback_fail.get(), "試験用の保存失敗");
            Ok(())
        };
        let mut state = Box::new(SettingsState::new(vec![sample_profile()], false, 0, true));
        state.on_change = Some(&mut save);
        let window = common::HiddenTestWindow::new(
            Some(window_proc),
            (&mut *state as *mut SettingsState<'_>).cast(),
        );
        common::set_text(state.alias, "未保存の変更");
        unsafe {
            SendMessageW(window.0, WM_CLOSE, 0, 0);
        }
        assert_ne!(unsafe { IsWindow(window.0) }, 0);
        assert_eq!(state.persisted.profiles[0].alias_name, "");
        assert!(common::window_text(state.status).contains("保存できません"));
        fail.set(false);
        common::set_text(state.escape_hold_duration, "1.05");
        unsafe {
            SendMessageW(window.0, WM_CLOSE, 0, 0);
        }
        assert_ne!(unsafe { IsWindow(window.0) }, 0);
        assert_eq!(state.persisted.escape_hold_duration_tenths, 0);
        common::set_text(state.escape_hold_duration, "0");
        unsafe {
            SendMessageW(window.0, WM_CLOSE, 0, 0);
        }
        assert_eq!(unsafe { IsWindow(window.0) }, 0);
        assert_eq!(state.persisted.profiles[0].alias_name, "未保存の変更");
    }

    #[test]
    fn all_settings_fit_the_standard_window_without_removed_explanations() {
        let mut state = Box::new(SettingsState::new(vec![sample_profile()], false, 0, true));
        let window = common::HiddenTestWindow::new(
            Some(window_proc),
            (&mut *state as *mut SettingsState<'_>).cast(),
        );
        let layout = SettingsLayout::new(940);
        assert_eq!(state.fixed.len(), layout.fixed.len());
        assert!(layout.height <= 620);
        for control in &state.fixed {
            let text = window_text(*control);
            assert!(
                !text.contains("一覧から選んで編集")
                    && !text.contains("生のURL")
                    && !text.contains("0.1～10.0秒")
            );
            assert!(!["保存", "キャンセル", "既定ルール一覧"].contains(&text.as_str()));
        }
        assert_ne!(unsafe { IsWindow(window.0) }, 0);
        for index in [5, 13, 17, 18, 19, 21, 22, 23] {
            let bounds = layout.fixed[index];
            let dc = unsafe { GetDC(window.0) };
            let previous = unsafe { SelectObject(dc, state.theme.font) };
            let text = wide(&window_text(state.fixed[index]));
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: bounds.width - 20,
                bottom: 0,
            };
            unsafe {
                DrawTextW(
                    dc,
                    text.as_ptr(),
                    text.len() as i32 - 1,
                    &mut rect,
                    DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX,
                );
                SelectObject(dc, previous);
                ReleaseDC(window.0, dc);
            }
            assert!(rect.bottom <= bounds.height, "項目{index}が収まらない");
        }
    }

    #[test]
    fn escape_hold_duration_uses_one_decimal_place() {
        assert_eq!(parse_escape_hold_duration("1.0"), Some(10));
        assert_eq!(parse_escape_hold_duration("0.1"), Some(1));
        assert_eq!(parse_escape_hold_duration("10"), Some(100));
        assert_eq!(parse_escape_hold_duration("1.05"), None);
        assert_eq!(parse_escape_hold_duration("0.0"), Some(0));
        assert_eq!(parse_escape_hold_duration("0"), Some(0));
        assert_eq!(parse_escape_hold_duration("10.1"), None);
        assert_eq!(format_escape_hold_duration(10), "1.0");
    }
}
