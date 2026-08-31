use crate::model::{DefaultRule, Profile};
use crate::rules;
use crate::ui::{show_error, show_information};
use crate::ui_support::{self as common, Page, Theme, wide, window_text};
use crate::url_input::ValidatedUrl;
use anyhow::ensure;
use std::mem::size_of;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::DRAWITEMSTRUCT;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetFocus, SetFocus, VK_ESCAPE, VK_RETURN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const WINDOW_CLASS: &str = "HttpSelector.RuleWindow";
const ID_LIST: i32 = 100;
const ID_PATTERN: i32 = 102;
const ID_SITE: i32 = 103;
const ID_LEADING_PATH: i32 = 104;
const ID_WILDCARD: i32 = 105;
const ID_NEW: i32 = 106;
const ID_SAVE: i32 = 107;
const ID_DELETE: i32 = 108;
const ID_CLOSE: i32 = 109;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuleDraft {
    pub profile_id: i64,
    pub url: ValidatedUrl,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RulesAction {
    Saved,
    Cancelled,
}

pub struct RulesOutcome {
    pub action: RulesAction,
    pub rules: Vec<DefaultRule>,
    pub pending_url: Option<ValidatedUrl>,
}

struct RulesState {
    profiles: Vec<Profile>,
    rules: Vec<DefaultRule>,
    selected: Option<usize>,
    selected_profile_id: Option<i64>,
    source_url: Option<ValidatedUrl>,
    pending_url: Option<ValidatedUrl>,
    dirty: bool,
    theme: Theme,
    page: Page,
    fixed: Vec<HWND>,
    list: HWND,
    profile: HWND,
    pattern: HWND,
    delete: HWND,
    action: RulesAction,
}

impl RulesState {
    fn receive_url(&mut self, url: ValidatedUrl) {
        self.pending_url = Some(url);
    }

    fn into_outcome(self) -> RulesOutcome {
        RulesOutcome {
            action: self.action,
            rules: self.rules,
            pending_url: self.pending_url,
        }
    }

    fn new(
        mut profiles: Vec<Profile>,
        mut rules: Vec<DefaultRule>,
        draft: Option<RuleDraft>,
    ) -> Self {
        profiles.sort_by_key(|profile| (profile.sort_order, profile.id));
        rules.sort_by_key(|rule| (rule.sort_order, rule.id));
        let (selected, selected_profile_id, source_url) = if let Some(draft) = draft {
            (None, Some(draft.profile_id), Some(draft.url))
        } else if rules.is_empty() {
            (None, None, None)
        } else {
            (Some(0), Some(rules[0].profile_id), None)
        };
        Self {
            profiles,
            rules,
            selected,
            selected_profile_id,
            source_url,
            pending_url: None,
            dirty: false,
            theme: Theme::new(96),
            page: Page::default(),
            fixed: Vec::new(),
            list: null_mut(),
            profile: null_mut(),
            pattern: null_mut(),
            delete: null_mut(),
            action: RulesAction::Cancelled,
        }
    }
}

pub fn run_rules(
    profiles: Vec<Profile>,
    rules: Vec<DefaultRule>,
    draft: Option<RuleDraft>,
) -> anyhow::Result<RulesOutcome> {
    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | WS_CLIPCHILDREN;
    let ex_style = WS_EX_TOPMOST | WS_EX_CONTROLPARENT;
    let bounds = common::initial_bounds(940, 620, style, ex_style);
    register_window_class()?;
    let pointer = Box::into_raw(Box::new(RulesState::new(profiles, rules, draft)));
    let window = unsafe {
        CreateWindowExW(
            ex_style,
            wide(&crate::app_identity::AppIdentity::current()?.window_class(WINDOW_CLASS)).as_ptr(),
            wide("HttpSelector 既定ルール").as_ptr(),
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
        anyhow::bail!(
            "既定ルール画面を作成できません: Win32 error {}",
            unsafe { GetLastError() }
        );
    }
    unsafe {
        ShowWindow(window, SW_SHOW);
        SetForegroundWindow(window);
    }
    let mut message: MSG = unsafe { std::mem::zeroed() };
    loop {
        let status = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
        ensure!(status != -1, "既定ルール画面のメッセージを取得できません");
        if status == 0 {
            break;
        }
        if common::handle_edit_shortcut(&message) {
            continue;
        }
        if message.message == WM_KEYDOWN {
            let focus = unsafe { GetFocus() };
            if message.wParam == VK_ESCAPE as usize {
                unsafe { SendMessageW(window, WM_CLOSE, 0, 0) };
                continue;
            }
            if message.wParam == VK_RETURN as usize && common::is_button(focus) {
                unsafe { SendMessageW(focus, BM_CLICK, 0, 0) };
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
    Ok(state.into_outcome())
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
            "既定ルール画面クラスを登録できません: {error}"
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
    let pointer = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) as *mut RulesState };
    match message {
        WM_CREATE if !pointer.is_null() => {
            let state = unsafe { &mut *pointer };
            state.theme = Theme::new(unsafe { GetDpiForWindow(window) }.max(96));
            create_controls(window, state);
            0
        }
        WM_SIZE if !pointer.is_null() => {
            let state = unsafe { &mut *pointer };
            layout_controls(window, state);
            0
        }
        WM_GETMINMAXINFO => {
            let info = unsafe { &mut *(lparam as *mut MINMAXINFO) };
            let dpi = unsafe { GetDpiForWindow(window) }.max(96);
            info.ptMinTrackSize.x = common::scale(940, dpi);
            info.ptMinTrackSize.y = common::scale(560, dpi);
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
            let field = lparam as HWND == state.profile
                || lparam as HWND == state.pattern
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
            common::draw_button(item);
            1
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
            // 外部から来たURLは編集内容と分離し、画面を閉じた後に選択処理へ戻す。
            state.receive_url(url);
            unsafe {
                ShowWindow(window, SW_RESTORE);
                SetForegroundWindow(window);
            }
            1
        }
        WM_COMMAND if !pointer.is_null() => {
            let state = unsafe { &mut *pointer };
            let id = (wparam & 0xffff) as i32;
            let notification = ((wparam >> 16) & 0xffff) as u16;
            if id == ID_LIST && notification == LBN_SELCHANGE as u16 {
                let selected = unsafe { SendMessageW(state.list, LB_GETCURSEL, 0, 0) };
                if selected != LB_ERR as isize {
                    state.selected = Some(selected as usize);
                    state.selected_profile_id = Some(state.rules[selected as usize].profile_id);
                    state.source_url = None;
                    load_editor(state);
                }
            } else if [ID_SITE, ID_LEADING_PATH, ID_WILDCARD].contains(&id)
                && notification == BN_CLICKED as u16
            {
                apply_pattern_preset(state, id);
            } else if id == ID_NEW && notification == BN_CLICKED as u16 {
                state.selected = None;
                state.selected_profile_id = None;
                state.source_url = None;
                load_editor(state);
            } else if id == ID_SAVE && notification == BN_CLICKED as u16 {
                save_rule(state);
            } else if id == ID_DELETE && notification == BN_CLICKED as u16 {
                delete_selected_rule(state);
            } else if id == ID_CLOSE && notification == BN_CLICKED as u16 {
                close_window(window, state);
            }
            0
        }
        WM_CLOSE => {
            if !pointer.is_null() {
                close_window(window, unsafe { &mut *pointer });
            } else {
                unsafe { DestroyWindow(window) };
            }
            0
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            0
        }
        WM_NCDESTROY => unsafe {
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
            DefWindowProcW(window, message, wparam, lparam)
        },
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

fn create_controls(window: HWND, state: &mut RulesState) {
    let font = state.theme.font;
    let title = common::label(window, "既定ルール一覧", state.theme.title);
    let subtitle = common::label(
        window,
        "URLに一致したルールのプロファイルを自動選択します。",
        font,
    );
    let list_label = common::label(window, "登録済みルール", state.theme.bold);
    state.list = common::control(
        window,
        "LISTBOX",
        "",
        ID_LIST,
        WS_TABSTOP | WS_BORDER | WS_VSCROLL | LBS_NOTIFY as u32,
        font,
    );
    let profile_label = common::label(window, "対象プロファイル", state.theme.bold);
    state.profile = common::edit(window, 0, false, true, font);
    let pattern_label = common::label(window, "URLパターン", state.theme.bold);
    state.pattern = common::edit(window, ID_PATTERN, false, false, font);
    let preset_label = common::label(window, "パターン作成（URL指定時のみ）", state.theme.bold);
    let site = common::button(window, "サイト全体", ID_SITE, font);
    let leading_path = common::button(window, "先頭パス", ID_LEADING_PATH, font);
    let wildcard = common::button(window, "ワイルドカード", ID_WILDCARD, font);
    let new_rule = common::button(window, "新規", ID_NEW, font);
    let save = common::button(window, "保存", ID_SAVE, state.theme.bold);
    state.delete = common::button(window, "削除", ID_DELETE, font);
    let close = common::button(window, "閉じる", ID_CLOSE, font);
    state.fixed = vec![
        title,
        subtitle,
        list_label,
        state.list,
        profile_label,
        state.profile,
        pattern_label,
        state.pattern,
        preset_label,
        site,
        leading_path,
        wildcard,
        new_rule,
        save,
        state.delete,
        close,
    ];
    layout_controls(window, state);
    refresh_list(state);
    load_editor(state);
}

fn layout_controls(window: HWND, state: &mut RulesState) {
    let width = Page::width(window, state.theme.dpi).max(860);
    state.page.controls.clear();
    let content = width - 48;
    let left_width = 320;
    let right_x = 24 + left_width + 24;
    let right_width = content - left_width - 24;
    let bounds = [
        common::Bounds::new(24, 18, content, 34),
        common::Bounds::new(24, 62, content, 24),
        common::Bounds::new(24, 100, left_width, 24),
        common::Bounds::new(24, 130, left_width, 350),
        common::Bounds::new(right_x, 100, right_width, 24),
        common::Bounds::new(right_x, 130, right_width, 36),
        common::Bounds::new(right_x, 188, right_width, 24),
        common::Bounds::new(right_x, 218, right_width, 36),
        common::Bounds::new(right_x, 272, right_width, 24),
        common::Bounds::new(right_x, 304, 112, 36),
        common::Bounds::new(right_x + 124, 304, 112, 36),
        common::Bounds::new(right_x + 248, 304, right_width - 248, 36),
        common::Bounds::new(right_x, 420, 112, 36),
        common::Bounds::new(right_x + 124, 420, 112, 36),
        common::Bounds::new(right_x + 248, 420, 112, 36),
        common::Bounds::new(right_x + 372, 420, right_width - 372, 36),
    ];
    for (control, bound) in state.fixed.iter().zip(bounds) {
        state.page.place(*control, bound);
    }
    state.page.height = 490;
    state.page.apply(window, state.theme.dpi);
}

fn refresh_list(state: &mut RulesState) {
    unsafe {
        SendMessageW(state.list, LB_RESETCONTENT, 0, 0);
        for rule in &state.rules {
            SendMessageW(
                state.list,
                LB_ADDSTRING,
                0,
                wide(&list_text(rule, &state.profiles)).as_ptr() as isize,
            );
        }
        if let Some(selected) = state.selected.filter(|index| *index < state.rules.len()) {
            SendMessageW(state.list, LB_SETCURSEL, selected, 0);
        }
    }
}

fn load_editor(state: &RulesState) {
    let profile_id = state
        .selected_profile_id
        .and_then(|id| state.profiles.iter().find(|profile| profile.id == id));
    common::set_text(
        state.profile,
        profile_id
            .map(|profile| profile.display_name().to_owned())
            .unwrap_or_else(|| "プロファイルを選択してください".to_owned())
            .as_str(),
    );
    common::set_text(
        state.pattern,
        state
            .selected
            .and_then(|index| state.rules.get(index))
            .map(|rule| rule.pattern.as_str())
            .or_else(|| state.source_url.as_ref().map(ValidatedUrl::original))
            .unwrap_or_default(),
    );
    unsafe { EnableWindow(state.delete, i32::from(state.selected.is_some())) };
}

fn apply_pattern_preset(state: &mut RulesState, id: i32) {
    let Some(url) = state.source_url.as_ref() else {
        show_information(
            "HttpSelector 既定ルール",
            "新規ルール登録時のURLから作成できます。",
        );
        return;
    };
    let pattern = match id {
        ID_SITE => rules::site_pattern(url),
        ID_LEADING_PATH => rules::leading_path_pattern(url),
        ID_WILDCARD => rules::wildcard_pattern(url),
        _ => return,
    };
    common::set_text(state.pattern, &pattern);
}

fn save_rule(state: &mut RulesState) {
    let Some(profile_id) = state.selected_profile_id else {
        show_error(
            "HttpSelector 既定ルール",
            "対象プロファイルを選択してください。",
        );
        return;
    };
    let pattern = window_text(state.pattern).trim().to_owned();
    if pattern.is_empty() || pattern.chars().any(char::is_control) {
        show_error("HttpSelector 既定ルール", "URLパターンを入力してください。");
        unsafe { SetFocus(state.pattern) };
        return;
    }
    if let Some(selected) = state.selected {
        if let Some(rule) = state.rules.get_mut(selected) {
            rule.profile_id = profile_id;
            rule.pattern = pattern;
        }
    } else {
        state.rules.push(DefaultRule {
            id: 0,
            profile_id,
            pattern,
            enabled: true,
            sort_order: state.rules.len() as i64,
            created_at: 0,
            updated_at: 0,
        });
        state.selected = Some(state.rules.len() - 1);
    }
    state.selected_profile_id = Some(profile_id);
    state.source_url = None;
    state.dirty = true;
    normalize_sort_order(&mut state.rules);
    refresh_list(state);
    load_editor(state);
}

fn delete_selected_rule(state: &mut RulesState) {
    let Some(selected) = state.selected else {
        return;
    };
    if selected >= state.rules.len() {
        return;
    }
    state.rules.remove(selected);
    state.selected = selected
        .checked_sub(1)
        .filter(|index| *index < state.rules.len());
    state.selected_profile_id = state
        .selected
        .and_then(|index| state.rules.get(index).map(|rule| rule.profile_id));
    state.dirty = true;
    normalize_sort_order(&mut state.rules);
    refresh_list(state);
    load_editor(state);
}

fn close_window(window: HWND, state: &mut RulesState) {
    state.action = if state.dirty {
        RulesAction::Saved
    } else {
        RulesAction::Cancelled
    };
    unsafe { DestroyWindow(window) };
}

fn normalize_sort_order(rules: &mut [DefaultRule]) {
    for (index, rule) in rules.iter_mut().enumerate() {
        rule.sort_order = index as i64;
    }
}

fn list_text(rule: &DefaultRule, profiles: &[Profile]) -> String {
    let profile = profiles
        .iter()
        .find(|profile| profile.id == rule.profile_id)
        .map(Profile::display_name)
        .unwrap_or("（削除されたプロファイル）");
    format!("{} → {}", profile, rule.pattern)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incoming_url_survives_close_without_replacing_the_rule_draft() {
        let original = ValidatedUrl::parse("https://example.com/original").unwrap();
        let incoming = ValidatedUrl::parse("https://example.net/incoming").unwrap();
        for action in [RulesAction::Saved, RulesAction::Cancelled] {
            let mut state = RulesState::new(
                vec![],
                vec![],
                Some(RuleDraft {
                    profile_id: 7,
                    url: original.clone(),
                }),
            );
            state.action = action;
            state.receive_url(incoming.clone());
            assert_eq!(state.source_url.as_ref(), Some(&original));
            assert_eq!(state.selected_profile_id, Some(7));
            let outcome = state.into_outcome();
            assert_eq!(outcome.pending_url, Some(incoming.clone()));
            assert_eq!(outcome.action, action);
        }
    }
}
