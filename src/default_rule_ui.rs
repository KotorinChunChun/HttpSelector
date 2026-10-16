use crate::app_title;
use crate::model::{DefaultRule, Profile};
use crate::rules;
use crate::ui::{show_error_for, show_information_for};
use crate::ui_layout::RulesLayout;
use crate::ui_support::{self as common, Page, Theme, wide, window_text};
use crate::url_input::ValidatedUrl;
use anyhow::ensure;
use std::mem::size_of;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{DRAWITEMSTRUCT, EM_SETREADONLY};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetFocus, SetFocus, VK_ESCAPE, VK_RETURN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const WINDOW_CLASS: &str = "HttpSelector.RuleWindow";
const ID_LIST: i32 = 100;
const ID_PROFILE: i32 = 101;
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
    let style =
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | WS_CLIPCHILDREN | WS_VSCROLL;
    let ex_style = WS_EX_CONTROLPARENT;
    let bounds = common::initial_bounds(940, 620, style, ex_style);
    register_window_class()?;
    let pointer = Box::into_raw(Box::new(RulesState::new(profiles, rules, draft)));
    let window = unsafe {
        CreateWindowExW(
            ex_style,
            wide(&crate::app_identity::AppIdentity::current()?.window_class(WINDOW_CLASS)).as_ptr(),
            wide(&app_title::default_rules()).as_ptr(),
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
    crate::management::set_active_window(window);
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
    if let Some(result) =
        unsafe { crate::management::handle_window_message(window, message, wparam) }
    {
        return result;
    }
    if message == WM_NCCREATE {
        let create = unsafe { &*(lparam as *const CREATESTRUCTW) };
        unsafe {
            SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
        }
    }
    let pointer = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) as *mut RulesState };
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
            layout_controls(window, state);
            0
        }
        WM_GETMINMAXINFO => {
            let info = unsafe { &mut *(lparam as *mut MINMAXINFO) };
            let dpi = unsafe { GetDpiForWindow(window) }.max(96);
            info.ptMinTrackSize.x = common::scale(400, dpi).min(info.ptMaxTrackSize.x);
            info.ptMinTrackSize.y = common::scale(320, dpi).min(info.ptMaxTrackSize.y);
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
            let Some(url) = (unsafe { crate::single_instance::received_url(lparam) }) else {
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
            } else if id == ID_PROFILE && notification == CBN_SELCHANGE as u16 {
                let selected = unsafe { SendMessageW(state.profile, CB_GETCURSEL, 0, 0) };
                state.selected_profile_id = usize::try_from(selected)
                    .ok()
                    .and_then(|index| state.profiles.get(index))
                    .map(|profile| profile.id);
            } else if [ID_SITE, ID_LEADING_PATH, ID_WILDCARD].contains(&id)
                && notification == BN_CLICKED as u16
            {
                apply_pattern_preset(window, state, id);
            } else if id == ID_NEW && notification == BN_CLICKED as u16 {
                state.selected = None;
                state.source_url = None;
                refresh_list(state);
                load_editor(state);
            } else if id == ID_SAVE && notification == BN_CLICKED as u16 {
                save_rule(window, state);
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
            crate::management::clear_active_window(window);
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
        "一致したルールで自動起動します。保存前に対象範囲を確認してください。",
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
    state.profile = common::control(
        window,
        "COMBOBOX",
        "",
        ID_PROFILE,
        WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST as u32,
        font,
    );
    for profile in &state.profiles {
        let text = format!(
            "{} | {}{}",
            profile.display_name(),
            profile.browser_type,
            if profile.available && profile.enabled {
                ""
            } else {
                "（利用不可）"
            }
        );
        unsafe {
            SendMessageW(
                state.profile,
                CB_ADDSTRING,
                0,
                wide(&text).as_ptr() as isize,
            )
        };
    }
    let pattern_label = common::label(window, "URLパターン（保存前に確認）", state.theme.bold);
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
    let layout = RulesLayout::new(Page::width(window, state.theme.dpi).max(360));
    state.page.controls.clear();
    for (control, mut bound) in state.fixed.iter().zip(layout.fixed) {
        if *control == state.profile {
            // コンボボックスの高さには展開時の一覧も含まれる。
            bound.height = 240;
        }
        state.page.place(*control, bound);
    }
    state.page.height = layout.height;
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
    let structured = state
        .selected
        .and_then(|index| state.rules.get(index))
        .and_then(|rule| {
            crate::association::StoredRule::decode(&rule.pattern)
                .ok()
                .flatten()
        });
    let selected = state
        .profiles
        .iter()
        .position(|profile| Some(profile.id) == state.selected_profile_id);
    unsafe {
        SendMessageW(
            state.profile,
            CB_SETCURSEL,
            selected.unwrap_or(usize::MAX),
            0,
        )
    };
    let initial_pattern = state.source_url.as_ref().map(rules::wildcard_pattern);
    let display = structured.as_ref().map(|rule| rule.regexes.join("  AND  "));
    common::set_text(
        state.pattern,
        state
            .selected
            .and_then(|index| state.rules.get(index))
            .map(|rule| display.as_deref().unwrap_or(&rule.pattern))
            .or(initial_pattern.as_deref())
            .unwrap_or_default(),
    );
    unsafe {
        SendMessageW(
            state.pattern,
            EM_SETREADONLY,
            usize::from(structured.is_some()),
            0,
        );
        EnableWindow(
            state.profile,
            i32::from(!structured.as_ref().is_some_and(|s| s.suppress_prompt)),
        );
        for id in [ID_SITE, ID_LEADING_PATH, ID_WILDCARD] {
            EnableWindow(
                GetDlgItem(GetParent(state.pattern), id),
                i32::from(structured.is_none()),
            );
        }
    }
    unsafe { EnableWindow(state.delete, i32::from(state.selected.is_some())) };
}

fn apply_pattern_preset(window: HWND, state: &mut RulesState, id: i32) {
    let Some(url) = state.source_url.as_ref() else {
        show_information_for(
            window,
            &app_title::default_rules(),
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

fn save_rule(window: HWND, state: &mut RulesState) {
    // 生成済み式を旧ワイルドカード入力として上書きしない。
    if let Some(selected) = state.selected
        && let Ok(Some(stored)) =
            crate::association::StoredRule::decode(&state.rules[selected].pattern)
    {
        if !stored.suppress_prompt {
            let Some(profile_id) = state.selected_profile_id else {
                return;
            };
            state.rules[selected].profile_id = profile_id;
        }
        state.dirty = true;
        refresh_list(state);
        return;
    }
    let Some(profile_id) = state.selected_profile_id else {
        show_error_for(
            window,
            &app_title::default_rules(),
            "対象プロファイルを選択してください。",
        );
        return;
    };
    let pattern = window_text(state.pattern).trim().to_owned();
    if pattern.is_empty() || pattern.chars().any(char::is_control) {
        show_error_for(
            window,
            &app_title::default_rules(),
            "URLパターンを入力してください。",
        );
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
    if crate::association::suppresses_prompt(&rule.pattern) {
        return format!(
            "質問しない → {}",
            crate::association::display_pattern(&rule.pattern)
        );
    }
    let profile = profiles
        .iter()
        .find(|profile| profile.id == rule.profile_id)
        .map(Profile::display_name)
        .unwrap_or("（削除されたプロファイル）");
    format!(
        "{} → {}",
        profile,
        crate::association::display_pattern(&rule.pattern)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_rules_are_not_rewritten_as_wildcards_and_suppression_is_identified() {
        let url = ValidatedUrl::parse("https://apps.powerapps.com/play?tenantId=123").unwrap();
        let parts = crate::association::UrlParts::new(&url);
        let pattern =
            crate::association::StoredRule::new(parts.condition(&parts.defaults()).unwrap(), true)
                .encode()
                .unwrap();
        let rule = DefaultRule {
            id: 1,
            profile_id: 0,
            pattern: pattern.clone(),
            enabled: true,
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
        };
        assert!(list_text(&rule, &[]).starts_with("質問しない → http(s)://"));
        let mut state = Box::new(RulesState::new(vec![], vec![rule], None));
        let window = common::HiddenTestWindow::new(
            Some(window_proc),
            (&mut *state as *mut RulesState).cast(),
        );
        assert!(!window_text(state.pattern).starts_with("urlparts-v1:"));
        assert_ne!(
            unsafe { GetWindowLongW(state.pattern, GWL_STYLE) } & ES_READONLY,
            0
        );
        save_rule(window.0, &mut state);
        assert_eq!(state.rules[0].pattern, pattern);
        delete_selected_rule(&mut state);
        assert!(state.rules.is_empty());
    }

    #[test]
    fn draft_and_preset_save_redacted_values_without_changing_existing_rules() {
        let temp = tempfile::tempdir().unwrap();
        let storage = crate::storage::Storage::new(temp.path().join("test.db"));
        storage.initialize().unwrap();
        storage
            .sync_detected(&[crate::model::DetectedProfile {
                browser_type: crate::model::BrowserType::Edge,
                profile_id: "Default".into(),
                detected_name: "検証用".into(),
                icon_path: None,
            }])
            .unwrap();
        let profiles = storage.load_all_profiles().unwrap();
        let profile_id = profiles[0].id;
        let legacy_pattern = "https://legacy.example.com/keep?manual=value";
        let existing = DefaultRule {
            id: 1,
            profile_id,
            pattern: legacy_pattern.into(),
            enabled: true,
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
        };
        let url = ValidatedUrl::parse("https://test-user-31:test-password-43@example.com/area/private-document-61?token=test-token-57#private-fragment-73").unwrap();
        let expected = "https://*@example.com/area/*?token=*#*";
        let mut state = Box::new(RulesState::new(
            profiles,
            vec![existing],
            Some(RuleDraft {
                profile_id,
                url: url.clone(),
            }),
        ));
        let window = common::HiddenTestWindow::new(
            Some(window_proc),
            (&mut *state as *mut RulesState).cast(),
        );
        assert_eq!(window_text(state.pattern), expected);
        assert_eq!(state.source_url.as_ref(), Some(&url));
        common::set_text(state.pattern, "手動編集中");
        unsafe { SendMessageW(window.0, WM_COMMAND, ID_WILDCARD as usize, 0) };
        assert_eq!(window_text(state.pattern), expected);
        unsafe { SendMessageW(window.0, WM_COMMAND, ID_SAVE as usize, 0) };
        assert_eq!(state.rules.len(), 2);
        assert_eq!(state.rules[0].pattern, legacy_pattern);
        assert_eq!(state.rules[1].pattern, expected);
        unsafe {
            SendMessageW(state.list, LB_SETCURSEL, 0, 0);
            SendMessageW(
                window.0,
                WM_COMMAND,
                ID_LIST as usize | ((LBN_SELCHANGE as usize) << 16),
                0,
            );
        }
        assert_eq!(window_text(state.pattern), legacy_pattern);
        storage.replace_default_rules(&state.rules).unwrap();
        assert_eq!(storage.load_default_rules().unwrap()[1].pattern, expected);
        let bytes = std::fs::read(storage.database_path()).unwrap();
        let contents = String::from_utf8_lossy(&bytes);
        for secret in [
            "test-user-31",
            "test-password-43",
            "private-document-61",
            "test-token-57",
            "private-fragment-73",
        ] {
            assert!(!contents.contains(secret));
        }
    }

    #[test]
    fn a_new_rule_can_select_a_profile_and_save_the_entered_pattern() {
        let profile = Profile {
            id: 8,
            browser_type: crate::model::BrowserType::Edge,
            profile_id: "Default".into(),
            detected_name: "検証用".into(),
            alias_name: String::new(),
            comment: String::new(),
            sort_order: 0,
            favorite: false,
            enabled: true,
            available: true,
            icon_path: None,
            created_at: 0,
            updated_at: 0,
        };
        let mut state = Box::new(RulesState::new(vec![profile], vec![], None));
        let window = common::HiddenTestWindow::new(
            Some(window_proc),
            (&mut *state as *mut RulesState).cast(),
        );
        unsafe {
            SendMessageW(window.0, WM_COMMAND, ID_NEW as usize, 0);
            SendMessageW(state.profile, CB_SETCURSEL, 0, 0);
            SendMessageW(
                window.0,
                WM_COMMAND,
                ID_PROFILE as usize | ((CBN_SELCHANGE as usize) << 16),
                0,
            );
        }
        common::set_text(state.pattern, "https://example.com/team/*");
        unsafe { SendMessageW(window.0, WM_COMMAND, ID_SAVE as usize, 0) };
        assert_eq!(state.rules.len(), 1);
        assert_eq!(state.rules[0].profile_id, 8);
        assert_eq!(state.rules[0].pattern, "https://example.com/team/*");
        unsafe { SendMessageW(window.0, WM_COMMAND, ID_CLOSE as usize, 0) };
        assert_eq!(state.action, RulesAction::Saved);
    }

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
