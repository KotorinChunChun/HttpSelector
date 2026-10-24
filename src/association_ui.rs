//! 連携フォーム。保存と起動を別イベントにし、保存後は閉じた設定から操作を選ぶ。
use crate::{
    app_title::WindowTitle,
    association::{Selection, StoredRule, UrlParts},
    model::{BrowserType, Profile},
    ui_support::{self as common, Bounds, Page, Theme},
    url_input::ValidatedUrl,
};
use anyhow::ensure;
use std::{
    mem::size_of,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Controls::*,
        HiDpi::GetDpiForWindow,
        Input::KeyboardAndMouse::{EnableWindow, GetFocus, SetFocus, VK_ESCAPE, VK_RETURN},
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};

const OPEN: i32 = 100;
const BACK: i32 = 101;
const EXPAND: i32 = 102;
const AUTO: i32 = 103;
const SKIP: i32 = 104;
const COPY: i32 = 105;
const SAVE: i32 = 106;
const SAVE_DOMAIN: i32 = 107;
const CHIP_BASE: i32 = 1000;
const NESTED_GROUP_BASE: i32 = 20000;
const NESTED_CHIP_BASE: i32 = 30000;

struct NestedGroup {
    path: Vec<usize>,
    parts: UrlParts,
    heading: HWND,
    chips: Vec<HWND>,
    expanded: bool,
}

fn selection_at<'a>(selection: &'a Selection, path: &[usize]) -> &'a Selection {
    path.iter()
        .fold(selection, |selection, index| &selection.nested[*index])
}
fn selection_at_mut<'a>(selection: &'a mut Selection, path: &[usize]) -> &'a mut Selection {
    path.iter()
        .fold(selection, |selection, index| &mut selection.nested[*index])
}
fn part_flags(selection: &Selection) -> impl Iterator<Item = bool> + '_ {
    std::iter::once(selection.host)
        .chain(selection.paths.iter().copied())
        .chain(selection.queries.iter().copied())
}
fn toggle_part(selection: &mut Selection, index: usize) {
    if index == 0 {
        selection.host = !selection.host;
    } else if index <= selection.paths.len() {
        selection.paths[index - 1] = !selection.paths[index - 1];
    } else if let Some(on) = selection.queries.get_mut(index - 1 - selection.paths.len()) {
        *on = !*on;
    }
}
fn part_labels(parts: &UrlParts) -> Vec<String> {
    std::iter::once(parts.host.clone())
        .chain(parts.paths.iter().map(|p| format!("/ {p}")))
        .chain(parts.queries.iter().cloned())
        .collect()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    Open,
    Back,
}
pub struct Outcome {
    pub action: Action,
    pub pending_url: Option<ValidatedUrl>,
}

struct State<'a> {
    parts: UrlParts,
    selection: Selection,
    profile_name: String,
    original_url: String,
    expanded: bool,
    suppress_prompt: bool,
    preset: Option<StoredRule>,
    legacy_pattern: Option<String>,
    action: Action,
    pending_url: Option<ValidatedUrl>,
    save: &'a mut dyn FnMut(&StoredRule) -> anyhow::Result<()>,
    theme: Theme,
    code_font: HFONT,
    page: Page,
    fixed: Vec<HWND>,
    chips: Vec<HWND>,
    nested: Vec<NestedGroup>,
    nested_bindings: Vec<(usize, usize)>,
    detail: Vec<HWND>,
    title: HWND,
    url: HWND,
    open: HWND,
    back: HWND,
    expand: HWND,
    scheme: HWND,
    query_prefix: HWND,
    auto: HWND,
    skip: HWND,
    regex_label: HWND,
    regex: HWND,
    copy: HWND,
    save_button: HWND,
    save_domain: HWND,
    status: HWND,
}

impl<'a> State<'a> {
    fn locked(&self) -> bool {
        self.preset.is_some() || self.legacy_pattern.is_some()
    }

    fn set_existing(&mut self, pattern: &str) -> anyhow::Result<()> {
        fn select(
            parts: &UrlParts,
            condition: Option<&crate::association::Condition>,
        ) -> Selection {
            Selection {
                host: condition.is_some_and(|c| c.host.is_some()),
                paths: (0..parts.paths.len())
                    .map(|i| condition.is_some_and(|c| c.paths.get(i).is_some_and(Option::is_some)))
                    .collect(),
                queries: parts
                    .queries
                    .iter()
                    .map(|q| condition.is_some_and(|c| c.queries.contains(q)))
                    .collect(),
                nested: parts
                    .nested
                    .iter()
                    .map(|(key, child)| {
                        select(
                            child,
                            condition.and_then(|c| {
                                c.nested
                                    .iter()
                                    .find(|n| &n.key == key)
                                    .map(|n| &*n.condition)
                            }),
                        )
                    })
                    .collect(),
            }
        }
        self.preset = StoredRule::decode(pattern)?;
        if let Some(rule) = &self.preset {
            self.selection = select(&self.parts, Some(&rule.condition));
            self.suppress_prompt = rule.suppress_prompt;
        } else {
            self.selection = select(&self.parts, None);
            self.legacy_pattern = Some(pattern.into());
        }
        Ok(())
    }

    fn new(
        url: &ValidatedUrl,
        profile_name: String,
        save: &'a mut dyn FnMut(&StoredRule) -> anyhow::Result<()>,
    ) -> Self {
        let parts = UrlParts::new(url);
        let selection = parts.defaults();
        Self {
            parts,
            selection,
            profile_name,
            original_url: url.original().into(),
            expanded: false,
            suppress_prompt: false,
            preset: None,
            legacy_pattern: None,
            action: Action::Back,
            pending_url: None,
            save,
            theme: Theme::new(96),
            code_font: null_mut(),
            page: Page::default(),
            fixed: vec![],
            chips: vec![],
            nested: vec![],
            nested_bindings: vec![],
            detail: vec![],
            title: null_mut(),
            url: null_mut(),
            open: null_mut(),
            back: null_mut(),
            expand: null_mut(),
            scheme: null_mut(),
            query_prefix: null_mut(),
            auto: null_mut(),
            skip: null_mut(),
            regex_label: null_mut(),
            regex: null_mut(),
            copy: null_mut(),
            save_button: null_mut(),
            save_domain: null_mut(),
            status: null_mut(),
        }
    }
    fn rule(&self) -> anyhow::Result<StoredRule> {
        if let Some(preset) = &self.preset {
            let mut rule = preset.clone();
            rule.suppress_prompt = self.suppress_prompt;
            return Ok(rule);
        }
        Ok(StoredRule::new(
            self.parts.condition(&self.selection)?,
            self.suppress_prompt,
        ))
    }
}

impl Drop for State<'_> {
    fn drop(&mut self) {
        if !self.code_font.is_null() {
            unsafe { DeleteObject(self.code_font) };
        }
    }
}

fn code_font(dpi: u32) -> HFONT {
    unsafe {
        CreateFontW(
            -common::scale(14, dpi),
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            0,
            0,
            CLEARTYPE_QUALITY as u32,
            0,
            common::wide("Consolas").as_ptr(),
        )
    }
}

pub fn profile_label(profile: &Profile) -> String {
    let browser = match profile.browser_type {
        BrowserType::Chrome => "Chrome",
        BrowserType::Edge => "Edge",
        BrowserType::Firefox => "Firefox",
    };
    format!("{browser} / {}", profile.display_name())
}

pub fn run(
    url: &ValidatedUrl,
    profile: &Profile,
    save: impl FnMut(&StoredRule) -> anyhow::Result<()>,
) -> anyhow::Result<Outcome> {
    run_with_existing(url, profile, None, save)
}

pub fn run_with_existing(
    url: &ValidatedUrl,
    profile: &Profile,
    existing: Option<&crate::model::DefaultRule>,
    mut save: impl FnMut(&StoredRule) -> anyhow::Result<()>,
) -> anyhow::Result<Outcome> {
    let _busy = crate::management::busy_guard();
    let class = common::wide(
        &crate::app_identity::AppIdentity::current()?
            .window_class("HttpSelector.AssociationWindow"),
    );
    let mut wc: WNDCLASSEXW = unsafe { std::mem::zeroed() };
    wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
    wc.lpfnWndProc = Some(window_proc);
    wc.hInstance = unsafe { GetModuleHandleW(null()) };
    wc.hCursor = unsafe { LoadCursorW(null_mut(), IDC_ARROW) };
    wc.lpszClassName = class.as_ptr();
    if unsafe { RegisterClassExW(&wc) } == 0 {
        ensure!(
            unsafe { GetLastError() } == 1410,
            "連携画面クラスを登録できません"
        );
    }
    let mut state = Box::new(State::new(url, profile_label(profile), &mut save));
    if let Some(existing) = existing {
        state.set_existing(&existing.pattern)?;
    }
    let style =
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | WS_CLIPCHILDREN | WS_VSCROLL;
    let bounds = common::initial_bounds(740, 310, style, WS_EX_CONTROLPARENT);
    let window = unsafe {
        CreateWindowExW(
            WS_EX_CONTROLPARENT,
            class.as_ptr(),
            common::wide(&WindowTitle::create(Some("リンクを開く"))).as_ptr(),
            style,
            bounds.x,
            bounds.y,
            bounds.width,
            bounds.height,
            null_mut(),
            null_mut(),
            wc.hInstance,
            (&mut *state as *mut State<'_>).cast(),
        )
    };
    ensure!(!window.is_null(), "連携画面を作成できません");
    unsafe {
        ShowWindow(window, SW_SHOW);
        SetForegroundWindow(window);
        SetFocus(state.open);
    }
    crate::management::set_active_window(window);
    let mut message: MSG = unsafe { std::mem::zeroed() };
    loop {
        let result = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
        if result <= 0 {
            ensure!(result == 0, "連携画面のメッセージを取得できません");
            break;
        }
        if common::handle_edit_shortcut(&message) {
            continue;
        }
        if message.message == WM_KEYDOWN {
            if message.wParam == VK_ESCAPE as usize {
                unsafe { SendMessageW(window, WM_CLOSE, 0, 0) };
                continue;
            }
            if message.wParam == VK_RETURN as usize && common::is_button(unsafe { GetFocus() }) {
                unsafe { SendMessageW(GetFocus(), BM_CLICK, 0, 0) };
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
            state
                .page
                .reveal(window, unsafe { GetFocus() }, state.theme.dpi);
        }
    }
    Ok(Outcome {
        action: state.action,
        pending_url: state.pending_url.take(),
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
    if message == WM_NCCREATE {
        let create = unsafe { &*(lparam as *const CREATESTRUCTW) };
        unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize) };
    }
    let pointer = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) as *mut State<'_> };
    if pointer.is_null() {
        return unsafe { DefWindowProcW(window, message, wparam, lparam) };
    }
    let state = unsafe { &mut *pointer };
    match message {
        WM_CREATE => {
            state.theme = Theme::new(unsafe { GetDpiForWindow(window) }.max(96));
            create_controls(window, state);
            0
        }
        WM_SIZE => {
            layout(window, state);
            0
        }
        WM_SYSCOMMAND if common::is_maximize_command(wparam) => 0,
        WM_GETMINMAXINFO => {
            let info = unsafe { &mut *(lparam as *mut MINMAXINFO) };
            let dpi = unsafe { GetDpiForWindow(window) }.max(96);
            info.ptMinTrackSize.x = common::scale(360, dpi).min(info.ptMaxTrackSize.x);
            info.ptMinTrackSize.y = common::scale(270, dpi).min(info.ptMaxTrackSize.y);
            0
        }
        WM_DPICHANGED => {
            let theme = Theme::new((wparam & 0xffff) as u32);
            common::update_fonts(&state.page, &state.theme, &theme);
            state.theme = theme;
            for control in state.fixed.iter().chain(&state.chips) {
                unsafe { SendMessageW(*control, WM_SETFONT, state.theme.font as usize, 1) };
            }
            for group in &state.nested {
                for control in std::iter::once(&group.heading).chain(&group.chips) {
                    unsafe { SendMessageW(*control, WM_SETFONT, state.theme.font as usize, 1) };
                }
            }
            unsafe { SendMessageW(state.title, WM_SETFONT, state.theme.title as usize, 1) };
            let previous = state.code_font;
            state.code_font = code_font(state.theme.dpi);
            unsafe {
                SendMessageW(state.regex, WM_SETFONT, state.code_font as usize, 1);
                DeleteObject(previous);
            }
            let r = unsafe { &*(lparam as *const RECT) };
            unsafe {
                SetWindowPos(
                    window,
                    null_mut(),
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )
            };
            0
        }
        WM_VSCROLL | WM_MOUSEWHEEL => {
            state.page.scroll(window, message, wparam, state.theme.dpi);
            0
        }
        WM_ERASEBKGND => {
            let mut r: RECT = unsafe { std::mem::zeroed() };
            unsafe {
                GetClientRect(window, &mut r);
                FillRect(wparam as _, &r, state.theme.background)
            };
            1
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN | WM_CTLCOLOREDIT => {
            unsafe {
                SetTextColor(wparam as _, common::TEXT);
                SetBkColor(wparam as _, common::BACKGROUND)
            };
            state.theme.background as isize
        }
        WM_DRAWITEM => {
            common::draw_button(unsafe { &*(lparam as *const DRAWITEMSTRUCT) });
            1
        }
        WM_COMMAND => {
            let id = (wparam & 0xffff) as i32;
            let notification = ((wparam >> 16) & 0xffff) as u16;
            if notification == BN_CLICKED as u16 {
                command(window, state, id);
            }
            0
        }
        WM_COPYDATA => {
            if let Some(url) = unsafe { crate::single_instance::received_url(lparam) } {
                if state.pending_url.is_some() {
                    return 0;
                }
                state.pending_url = Some(url);
                1
            } else {
                0
            }
        }
        WM_CLOSE => {
            unsafe { DestroyWindow(window) };
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

unsafe extern "system" fn chip_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    _: usize,
) -> LRESULT {
    match message {
        WM_PAINT => {
            let mut paint: PAINTSTRUCT = unsafe { std::mem::zeroed() };
            let dc = unsafe { BeginPaint(window, &mut paint) };
            paint_chip(window, dc);
            unsafe { EndPaint(window, &paint) };
            0
        }
        WM_PRINTCLIENT => {
            paint_chip(window, wparam as HDC);
            0
        }
        WM_ERASEBKGND => 1,
        WM_NCDESTROY => unsafe {
            RemoveWindowSubclass(window, Some(chip_proc), id);
            DefSubclassProc(window, message, wparam, lparam)
        },
        _ => unsafe { DefSubclassProc(window, message, wparam, lparam) },
    }
}

fn paint_chip(window: HWND, dc: HDC) {
    let mut item: DRAWITEMSTRUCT = unsafe { std::mem::zeroed() };
    item.hwndItem = window;
    item.hDC = dc;
    unsafe { GetClientRect(window, &mut item.rcItem) };
    if unsafe { GetFocus() } == window {
        item.itemState |= ODS_FOCUS;
    }
    if unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(window) } == 0 {
        item.itemState |= ODS_DISABLED;
    }
    if unsafe { SendMessageW(window, BM_GETSTATE, 0, 0) } as u32 & BST_PUSHED != 0 {
        item.itemState |= ODS_SELECTED;
    }
    let checked = unsafe { SendMessageW(window, BM_GETCHECK, 0, 0) } == BST_CHECKED as isize;
    common::draw_button_state(&item, checked, true);
}

fn create_controls(window: HWND, state: &mut State<'_>) {
    let font = state.theme.font;
    state.title = common::label(
        window,
        &format!("以下のURLを{} で開く", state.profile_name),
        state.theme.title,
    );
    state.url = common::edit(window, 0, true, true, font);
    common::set_text(state.url, &state.original_url);
    state.open = common::button(window, "開く", OPEN, state.theme.bold);
    state.back = common::button(window, "戻る", BACK, font);
    state.save_domain = common::button(window, "ドメインを保存", SAVE_DOMAIN, font);
    state.expand = common::button(window, "▶ 今後の動作を設定", EXPAND, font);
    state.scheme = common::label(window, "http(s)://", font);
    let labels = part_labels(&state.parts);
    for (index, label) in labels.iter().enumerate() {
        let chip = common::control(
            window,
            "BUTTON",
            &crate::nested_url::preview(label),
            CHIP_BASE + index as i32,
            WS_TABSTOP | BS_AUTOCHECKBOX as u32 | BS_PUSHLIKE as u32 | BS_MULTILINE as u32,
            font,
        );
        // 標準チェックボックスの操作・アクセシビリティを保ち、描画だけを共通化する。
        unsafe { SetWindowSubclass(chip, Some(chip_proc), 1, 0) };
        state.chips.push(chip);
    }
    fn groups(
        window: HWND,
        parts: &UrlParts,
        path: Vec<usize>,
        names: String,
        state: &mut State<'_>,
    ) {
        for (index, (key, child)) in parts.nested.iter().enumerate() {
            let mut child_path = path.clone();
            child_path.push(index);
            let title = format!("{names}{key} 内");
            let group_index = state.nested.len();
            let heading = common::button(
                window,
                &format!("▶ {title}"),
                NESTED_GROUP_BASE + group_index as i32,
                state.theme.font,
            );
            let mut chips = vec![];
            for (part_index, label) in part_labels(child).iter().enumerate() {
                let chip = common::control(
                    window,
                    "BUTTON",
                    &crate::nested_url::preview(label),
                    NESTED_CHIP_BASE + state.nested_bindings.len() as i32,
                    WS_TABSTOP | BS_AUTOCHECKBOX as u32 | BS_PUSHLIKE as u32 | BS_MULTILINE as u32,
                    state.theme.font,
                );
                unsafe { SetWindowSubclass(chip, Some(chip_proc), 1, 0) };
                chips.push(chip);
                state.nested_bindings.push((group_index, part_index));
            }
            state.nested.push(NestedGroup {
                path: child_path.clone(),
                parts: child.clone(),
                heading,
                chips,
                expanded: false,
            });
            groups(window, child, child_path, format!("{names}{key} → "), state);
        }
    }
    groups(window, &state.parts.clone(), vec![], String::new(), state);
    state.query_prefix = common::label(window, "?", font);
    state.auto = common::control(
        window,
        "BUTTON",
        &format!("次回から {} で自動的に開く", state.profile_name),
        AUTO,
        WS_TABSTOP | WS_GROUP | BS_AUTORADIOBUTTON as u32 | BS_MULTILINE as u32,
        font,
    );
    state.skip = common::control(
        window,
        "BUTTON",
        "今後、関連付けを質問しない",
        SKIP,
        WS_TABSTOP | BS_AUTORADIOBUTTON as u32,
        font,
    );
    state.regex_label = common::label(window, "保存する正規表現", font);
    state.regex = common::edit(window, 0, true, true, font);
    state.code_font = code_font(state.theme.dpi);
    unsafe { SendMessageW(state.regex, WM_SETFONT, state.code_font as usize, 0) };
    state.copy = common::button(window, "コピー", COPY, font);
    state.save_button = common::button(window, "保存", SAVE, state.theme.bold);
    state.status = common::label(window, "", font);
    state.detail = vec![
        state.scheme,
        state.query_prefix,
        state.auto,
        state.skip,
        state.regex_label,
        state.regex,
        state.copy,
        state.save_button,
    ];
    state.detail.extend(&state.chips);
    state.fixed = vec![
        state.title,
        state.url,
        state.open,
        state.back,
        state.expand,
        state.scheme,
        state.query_prefix,
        state.auto,
        state.skip,
        state.regex_label,
        state.regex,
        state.copy,
        state.save_button,
        state.status,
        state.save_domain,
    ];
    update(state);
    layout(window, state);
}

fn update(state: &State<'_>) {
    common::set_text(
        state.save_domain,
        if state.locked() {
            "登録先を保存"
        } else {
            "ドメインを保存"
        },
    );
    unsafe {
        ShowWindow(
            state.save_domain,
            if state.expanded { SW_HIDE } else { SW_SHOW },
        );
    }
    let flags = std::iter::once(state.selection.host)
        .chain(state.selection.paths.iter().copied())
        .chain(state.selection.queries.iter().copied());
    for (control, on) in state.chips.iter().zip(flags) {
        unsafe { EnableWindow(*control, (!state.locked()) as i32) };
        unsafe {
            SendMessageW(
                *control,
                BM_SETCHECK,
                if on { BST_CHECKED } else { BST_UNCHECKED } as usize,
                0,
            )
        };
        unsafe { InvalidateRect(*control, null(), 0) };
    }
    unsafe {
        SendMessageW(
            state.auto,
            BM_SETCHECK,
            if state.suppress_prompt {
                BST_UNCHECKED
            } else {
                BST_CHECKED
            } as usize,
            0,
        );
        SendMessageW(
            state.skip,
            BM_SETCHECK,
            if state.suppress_prompt {
                BST_CHECKED
            } else {
                BST_UNCHECKED
            } as usize,
            0,
        );
    }
    unsafe { EnableWindow(state.skip, state.legacy_pattern.is_none() as i32) };
    match state.rule() {
        Ok(rule) => {
            common::set_text(state.regex, &rule.display_regexes());
            common::set_text(
                state.regex_label,
                if rule.regexes.len() > 1 {
                    "正規表現（すべてに一致）"
                } else {
                    "保存する正規表現"
                },
            );
        }
        Err(error) => common::set_text(state.status, &format!("{error:#}")),
    }
    if state.locked() {
        common::set_text(state.regex_label, "既存条件（維持して登録先を置き換え）");
        if let Some(pattern) = &state.legacy_pattern {
            common::set_text(state.regex, pattern);
        }
    }
    common::set_text(
        state.expand,
        if state.expanded {
            "▼ 今後の動作を設定"
        } else {
            "▶ 今後の動作を設定"
        },
    );
    for control in &state.detail {
        unsafe { ShowWindow(*control, if state.expanded { SW_SHOW } else { SW_HIDE }) };
    }
    if state.parts.queries.is_empty() {
        unsafe { ShowWindow(state.query_prefix, SW_HIDE) };
    }
    for group in &state.nested {
        let ancestors_visible = state
            .nested
            .iter()
            .filter(|parent| {
                parent.path.len() < group.path.len() && group.path.starts_with(&parent.path)
            })
            .all(|parent| parent.expanded);
        let visible = state.expanded && ancestors_visible;
        let current = common::window_text(group.heading);
        let title = current.trim_start_matches(['▶', '▼', ' ']);
        common::set_text(
            group.heading,
            &format!("{} {title}", if group.expanded { "▼" } else { "▶" }),
        );
        unsafe { ShowWindow(group.heading, if visible { SW_SHOW } else { SW_HIDE }) };
        for (chip, on) in group
            .chips
            .iter()
            .zip(part_flags(selection_at(&state.selection, &group.path)))
        {
            unsafe { EnableWindow(*chip, (!state.locked()) as i32) };
            unsafe {
                SendMessageW(
                    *chip,
                    BM_SETCHECK,
                    if on { BST_CHECKED } else { BST_UNCHECKED } as usize,
                    0,
                );
                InvalidateRect(*chip, null(), 0);
                ShowWindow(
                    *chip,
                    if visible && group.expanded {
                        SW_SHOW
                    } else {
                        SW_HIDE
                    },
                );
            }
        }
    }
}

fn command(window: HWND, state: &mut State<'_>, id: i32) {
    if state.locked() && (id >= NESTED_CHIP_BASE || (CHIP_BASE..NESTED_GROUP_BASE).contains(&id)) {
        return;
    }
    if state.legacy_pattern.is_some() && id == SKIP {
        return;
    }
    match id {
        OPEN => {
            state.action = Action::Open;
            unsafe { DestroyWindow(window) };
        }
        BACK => {
            unsafe { DestroyWindow(window) };
        }
        EXPAND => {
            state.expanded = !state.expanded;
            update(state);
            layout(window, state);
            fit_height(window, state);
        }
        AUTO | SKIP => {
            state.suppress_prompt = id == SKIP;
            update(state);
        }
        SAVE | SAVE_DOMAIN => {
            let mut quick_selection = None;
            let rule = if id == SAVE_DOMAIN && !state.locked() {
                let mut selection = state.parts.defaults();
                if ValidatedUrl::parse(state.original_url.clone())
                    .is_ok_and(|url| crate::nested_url::is_teams_link(&url))
                {
                    for (index, (key, _)) in state.parts.nested.iter().enumerate() {
                        if key == "url" {
                            selection.nested[index].host = true;
                        }
                    }
                }
                let rule = state
                    .parts
                    .condition(&selection)
                    .map(|condition| StoredRule::new(condition, false));
                quick_selection = Some(selection);
                rule
            } else {
                state.rule()
            };
            let result = rule.and_then(|rule| {
                let url = ValidatedUrl::parse(state.original_url.clone())?;
                ensure!(state.locked() || !crate::nested_url::is_teams_link(&url) || rule.suppress_prompt || rule.matches_teams_target(&url), "自動振り分けの条件をurl内で1つ以上ONにしてください（ドメイン・パスなど）。TeamsCIDは必須ではありません。");
                (state.save)(&rule)
            });
            match result {
                Ok(()) => {
                    if let Some(selection) = quick_selection {
                        state.selection = selection;
                        state.suppress_prompt = false;
                    }
                    state.expanded = false;
                    common::set_text(state.status, "保存しました。");
                    update(state);
                    state.page.offset = 0;
                    layout(window, state);
                    fit_height(window, state);
                    unsafe { SetFocus(state.open) };
                }
                Err(error) => {
                    common::set_text(state.status, &format!("保存できません: {error:#}"));
                    layout(window, state);
                    state.page.reveal(window, state.status, state.theme.dpi);
                }
            }
        }
        COPY => {
            unsafe {
                SendMessageW(state.regex, EM_SETSEL, 0, -1);
                SendMessageW(state.regex, WM_COPY, 0, 0);
                SendMessageW(state.regex, EM_SETSEL, 0, 0)
            };
            common::set_text(state.status, "正規表現をコピーしました。");
        }
        _ if id >= NESTED_CHIP_BASE => {
            if let Some(&(group_index, part)) =
                state.nested_bindings.get((id - NESTED_CHIP_BASE) as usize)
            {
                toggle_part(
                    selection_at_mut(&mut state.selection, &state.nested[group_index].path),
                    part,
                );
                common::set_text(state.status, "");
                update(state);
            }
        }
        _ if id >= NESTED_GROUP_BASE => {
            if let Some(group) = state.nested.get_mut((id - NESTED_GROUP_BASE) as usize) {
                group.expanded = !group.expanded;
            }
            update(state);
            layout(window, state);
            fit_height(window, state);
        }
        _ if id >= CHIP_BASE => {
            let index = (id - CHIP_BASE) as usize;
            if index == 0 {
                state.selection.host = !state.selection.host;
            } else if index <= state.selection.paths.len() {
                state.selection.paths[index - 1] = !state.selection.paths[index - 1];
            } else if let Some(on) = state
                .selection
                .queries
                .get_mut(index - 1 - state.selection.paths.len())
            {
                *on = !*on;
            }
            common::set_text(state.status, "");
            update(state);
        }
        _ => {}
    }
}

fn measure(window: HWND, text: &str, width: i32, font: HFONT, dpi: u32) -> (i32, i32) {
    let dc = unsafe { GetDC(window) };
    let previous = unsafe { SelectObject(dc, font) };
    let text = common::wide(text);
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: common::scale(width, dpi),
        bottom: 0,
    };
    unsafe {
        DrawTextW(
            dc,
            text.as_ptr(),
            (text.len() - 1) as i32,
            &mut rect,
            DT_CALCRECT | DT_WORDBREAK | DT_EDITCONTROL | DT_NOPREFIX,
        );
        SelectObject(dc, previous);
        ReleaseDC(window, dc);
    }
    (
        (rect.right * 96 / dpi as i32).max(1),
        (rect.bottom * 96 / dpi as i32).max(22),
    )
}

// GUID等の長い単語も欠けないように、条件値を変えず表示だけを折り返す。
fn wrapped(window: HWND, text: &str, width: i32, font: HFONT, dpi: u32) -> String {
    let dc = unsafe { GetDC(window) };
    let previous = unsafe { SelectObject(dc, font) };
    let limit = common::scale(width, dpi);
    let mut result = String::new();
    let mut line = String::new();
    for ch in text.chars() {
        let candidate = format!("{line}{ch}");
        let wide = common::wide(&candidate);
        let mut size: SIZE = unsafe { std::mem::zeroed() };
        unsafe { GetTextExtentPoint32W(dc, wide.as_ptr(), (wide.len() - 1) as i32, &mut size) };
        if size.cx > limit && !line.is_empty() {
            result.push_str(&line);
            result.push('\n');
            line.clear();
        }
        line.push(ch);
    }
    result.push_str(&line);
    unsafe {
        SelectObject(dc, previous);
        ReleaseDC(window, dc);
    }
    result
}

fn layout(window: HWND, state: &mut State<'_>) {
    let width = Page::width(window, state.theme.dpi).max(300);
    let margin = 20;
    let content = width - margin * 2;
    state.page.controls.clear();
    let title = common::window_text(state.title);
    let title_h = measure(window, &title, content, state.theme.title, state.theme.dpi).1 + 8;
    state
        .page
        .place(state.title, Bounds::new(margin, 16, content, title_h));
    let url_h = (measure(
        window,
        &state.original_url,
        content - 20,
        state.theme.font,
        state.theme.dpi,
    )
    .1 + 16)
        .clamp(56, 144);
    let mut y = 24 + title_h;
    state
        .page
        .place(state.url, Bounds::new(margin, y, content, url_h));
    y += url_h + 14;
    state
        .page
        .place(state.open, Bounds::new(margin, y, 100, 36));
    state
        .page
        .place(state.back, Bounds::new(margin + 110, y, 90, 36));
    if !state.expanded {
        if content >= 380 {
            state
                .page
                .place(state.save_domain, Bounds::new(margin + 210, y, 170, 36));
        } else {
            y += 46;
            state.page.place(
                state.save_domain,
                Bounds::new(margin, y, 170.min(content), 36),
            );
        }
    }
    y += 52;
    state
        .page
        .place(state.expand, Bounds::new(margin, y, content, 36));
    y += 48;
    if state.expanded {
        state
            .page
            .place(state.scheme, Bounds::new(margin, y, content, 24));
        y += 28;
        let mut x = margin;
        let mut row_height = 0;
        for (index, control) in state
            .chips
            .iter()
            .take(1 + state.parts.paths.len())
            .enumerate()
        {
            let text = if index == 0 {
                state.parts.host.clone()
            } else {
                format!("/ {}", state.parts.paths[index - 1])
            };
            let text = crate::nested_url::preview(&text);
            let desired = measure(window, &text, 32760, state.theme.font, state.theme.dpi).0 + 28;
            let w = desired.clamp(48, content);
            let text = wrapped(window, &text, w - 24, state.theme.font, state.theme.dpi);
            common::set_text(*control, &text);
            let h =
                (measure(window, &text, w - 24, state.theme.font, state.theme.dpi).1 + 18).max(36);
            if x > margin && x + w > width - margin {
                x = margin;
                y += row_height + 8;
                row_height = 0;
            }
            state.page.place(*control, Bounds::new(x, y, w, h));
            x += w + 8;
            row_height = row_height.max(h);
        }
        y += row_height + 12;
        if !state.parts.queries.is_empty() {
            state
                .page
                .place(state.query_prefix, Bounds::new(margin, y, content, 24));
            y += 28;
            let columns = if content >= 560 { 2 } else { 1 };
            let w = (content - (columns - 1) * 8) / columns;
            for (row_index, row) in state.chips[1 + state.parts.paths.len()..]
                .chunks(columns as usize)
                .enumerate()
            {
                for (index, control) in row.iter().enumerate() {
                    let text = wrapped(
                        window,
                        &crate::nested_url::preview(
                            &state.parts.queries[row_index * columns as usize + index],
                        ),
                        w - 24,
                        state.theme.font,
                        state.theme.dpi,
                    );
                    common::set_text(*control, &text);
                }
                let h = row
                    .iter()
                    .map(|c| {
                        measure(
                            window,
                            &common::window_text(*c),
                            w - 24,
                            state.theme.font,
                            state.theme.dpi,
                        )
                        .1 + 18
                    })
                    .max()
                    .unwrap_or(36)
                    .max(36);
                for (index, control) in row.iter().enumerate() {
                    state.page.place(
                        *control,
                        Bounds::new(margin + index as i32 * (w + 8), y, w, h),
                    );
                }
                y += h + 8;
            }
            y += 8;
        }
        for group in &state.nested {
            if !state
                .nested
                .iter()
                .filter(|parent| {
                    parent.path.len() < group.path.len() && group.path.starts_with(&parent.path)
                })
                .all(|parent| parent.expanded)
            {
                continue;
            }
            let indent = (group.path.len() as i32 * 12).min(48);
            let left = margin + indent;
            let available = content - indent;
            state
                .page
                .place(group.heading, Bounds::new(left, y, available, 36));
            y += 44;
            if !group.expanded {
                continue;
            }
            for (index, (chip, label)) in group
                .chips
                .iter()
                .zip(part_labels(&group.parts))
                .enumerate()
            {
                let label = if index > group.parts.paths.len() {
                    format!(
                        "{} {}",
                        if index == group.parts.paths.len() + 1 {
                            "?"
                        } else {
                            "&"
                        },
                        crate::nested_url::preview(&label)
                    )
                } else {
                    crate::nested_url::preview(&label)
                };
                let text = wrapped(
                    window,
                    &label,
                    available - 24,
                    state.theme.font,
                    state.theme.dpi,
                );
                common::set_text(*chip, &text);
                let height = (measure(
                    window,
                    &text,
                    available - 24,
                    state.theme.font,
                    state.theme.dpi,
                )
                .1 + 18)
                    .max(36);
                state
                    .page
                    .place(*chip, Bounds::new(left, y, available, height));
                y += height + 8;
            }
            y += 8;
        }
        let auto_h = measure(
            window,
            &common::window_text(state.auto),
            content - 30,
            state.theme.font,
            state.theme.dpi,
        )
        .1
        .max(28)
            + 8;
        state
            .page
            .place(state.auto, Bounds::new(margin, y, content, auto_h));
        y += auto_h + 4;
        state
            .page
            .place(state.skip, Bounds::new(margin, y, content, 32));
        y += 48;
        state
            .page
            .place(state.regex_label, Bounds::new(margin, y, content - 90, 32));
        state
            .page
            .place(state.copy, Bounds::new(width - margin - 80, y, 80, 32));
        y += 38;
        state
            .page
            .place(state.regex, Bounds::new(margin, y, content, 120));
        y += 132;
        state
            .page
            .place(state.save_button, Bounds::new(margin, y, 100, 36));
        y += 48;
    }
    let status_h = measure(
        window,
        &common::window_text(state.status),
        content,
        state.theme.font,
        state.theme.dpi,
    )
    .1 + 4;
    state
        .page
        .place(state.status, Bounds::new(margin, y, content, status_h));
    state.page.height = y + status_h + 16;
    state.page.apply(window, state.theme.dpi);
}

fn fit_height(window: HWND, state: &State<'_>) {
    let mut frame: RECT = unsafe { std::mem::zeroed() };
    unsafe { GetWindowRect(window, &mut frame) };
    let mut client: RECT = unsafe { std::mem::zeroed() };
    unsafe { GetClientRect(window, &mut client) };
    let monitor = unsafe { MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST) };
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = size_of::<MONITORINFO>() as u32;
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return;
    }
    let height = (common::scale(state.page.height, state.theme.dpi) + frame.bottom
        - frame.top
        - client.bottom)
        .min(info.rcWork.bottom - info.rcWork.top);
    let top = frame
        .top
        .min(info.rcWork.bottom - height)
        .max(info.rcWork.top);
    unsafe {
        SetWindowPos(
            window,
            null_mut(),
            frame.left,
            top,
            frame.right - frame.left,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapsed_domain_save_does_not_save_parameters_or_open_the_url() {
        let url =
            ValidatedUrl::parse("https://example.com/private/document?token=not-saved#private")
                .unwrap();
        let mut saved = vec![];
        let mut save = |rule: &StoredRule| {
            saved.push(rule.clone());
            Ok(())
        };
        let mut state = Box::new(State::new(&url, "Edge / テスト".into(), &mut save));
        let window = common::HiddenTestWindow::new(
            Some(window_proc),
            (&mut *state as *mut State<'_>).cast(),
        );
        assert!(!state.expanded);
        command(window.0, &mut state, SAVE_DOMAIN);
        assert!(!state.expanded);
        assert_eq!(state.action, Action::Back);
        assert_ne!(unsafe { IsWindow(window.0) }, 0);
        drop(state);
        drop(save);
        assert_eq!(saved.len(), 1);
        let encoded = saved[0].encode().unwrap();
        assert!(
            !encoded.contains("not-saved")
                && !encoded.contains("document")
                && !encoded.contains("private")
        );
        assert!(saved[0].matches("https://example.com/another?different=value"));
        assert!(!saved[0].matches("https://other.example.com/"));
    }

    #[test]
    fn collapsed_save_preserves_existing_conditions_and_nested_target_guard() {
        let url = ValidatedUrl::parse("https://example.com/a?tenant=chosen&token=secret").unwrap();
        let parts = UrlParts::new(&url);
        let mut selection = parts.defaults();
        selection.queries[0] = true;
        let original = StoredRule::new(parts.condition(&selection).unwrap(), false);
        let mut saved = vec![];
        let mut save = |rule: &StoredRule| {
            saved.push(rule.clone());
            Ok(())
        };
        let mut state = Box::new(State::new(&url, "Edge / 別".into(), &mut save));
        state.set_existing(&original.encode().unwrap()).unwrap();
        let window = common::HiddenTestWindow::new(
            Some(window_proc),
            (&mut *state as *mut State<'_>).cast(),
        );
        assert_eq!(common::window_text(state.save_domain), "登録先を保存");
        command(window.0, &mut state, SAVE_DOMAIN);
        drop(state);
        drop(save);
        assert_eq!(saved[0], original);

        let wrapper = ValidatedUrl::parse("https://teams.public.onecdn.static.microsoft/evergreen-assets/safelinks/2/atp-safelinks.html?url=https%3A%2F%2Fexample.sharepoint.com%2Fsite%3FTeamsCID%3Dnot-stable&pc=secret").unwrap();
        let mut save = |rule: &StoredRule| {
            assert!(rule.matches_teams_target(&wrapper));
            assert!(!rule.encode().unwrap().contains("not-stable"));
            Ok(())
        };
        let mut state = Box::new(State::new(&wrapper, "Edge / テスト".into(), &mut save));
        let window = common::HiddenTestWindow::new(
            Some(window_proc),
            (&mut *state as *mut State<'_>).cast(),
        );
        command(window.0, &mut state, SAVE_DOMAIN);
        assert!(common::window_text(state.status).contains("保存しました"));
        assert!(!state.expanded);
    }

    #[test]
    fn legacy_replacement_preserves_pattern_and_back_never_saves() {
        let url = ValidatedUrl::parse("https://example.com/a").unwrap();
        let mut calls = 0;
        let mut save = |_: &StoredRule| {
            calls += 1;
            Ok(())
        };
        let mut state = Box::new(State::new(&url, "Edge / 変更先".into(), &mut save));
        state.set_existing("https://example.com/*").unwrap();
        let window = common::HiddenTestWindow::new(
            Some(window_proc),
            (&mut *state as *mut State<'_>).cast(),
        );
        command(window.0, &mut state, EXPAND);
        assert_eq!(common::window_text(state.regex), "https://example.com/*");
        command(window.0, &mut state, SKIP);
        assert!(!state.suppress_prompt);
        command(window.0, &mut state, BACK);
        assert_eq!(state.action, Action::Back);
        drop(state);
        drop(save);
        assert_eq!(calls, 0);
    }

    #[test]
    fn existing_rule_keeps_conditions_and_save_does_not_open_or_close() {
        let url =
            ValidatedUrl::parse("https://example.com/a?tenant=chosen&token=not-saved").unwrap();
        let parts = UrlParts::new(&url);
        let mut selected = parts.defaults();
        selected.queries[0] = true;
        let original = StoredRule::new(parts.condition(&selected).unwrap(), true);
        let mut saved = vec![];
        let mut save = |rule: &StoredRule| {
            saved.push(rule.clone());
            Ok(())
        };
        let mut state = Box::new(State::new(&url, "Edge / 別の登録先".into(), &mut save));
        state.set_existing(&original.encode().unwrap()).unwrap();
        let window = common::HiddenTestWindow::new(
            Some(window_proc),
            (&mut *state as *mut State<'_>).cast(),
        );
        command(window.0, &mut state, EXPAND);
        assert!(common::window_text(state.regex).contains("chosen"));
        assert!(!common::window_text(state.regex).contains("not-saved"));
        command(window.0, &mut state, CHIP_BASE);
        assert_eq!(state.rule().unwrap(), original);
        command(window.0, &mut state, AUTO);
        command(window.0, &mut state, SAVE);
        assert!(!state.expanded);
        assert_eq!(state.action, Action::Back);
        assert_ne!(unsafe { IsWindow(window.0) }, 0);
        command(window.0, &mut state, BACK);
        drop(state);
        drop(save);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].condition, original.condition);
        assert!(!saved[0].suppress_prompt);
    }

    #[test]
    fn teams_saves_host_or_personal_path_without_selecting_cid() {
        for host_only in [false, true] {
            let mut url = url::Url::parse("https://teams.public.onecdn.static.microsoft/evergreen-assets/safelinks/2/atp-safelinks.html").unwrap();
            url.query_pairs_mut().append_pair("url", "https://tenant-my.sharepoint.com/:f:/g/personal/person_example_com/document?TeamsCID=changing&secret=private");
            let input = ValidatedUrl::parse(url.to_string()).unwrap();
            let mut saved = vec![];
            let mut save = |rule: &StoredRule| {
                saved.push(rule.clone());
                Ok(())
            };
            let mut state = Box::new(State::new(&input, "Edge / 仕事".into(), &mut save));
            let window = common::HiddenTestWindow::new(
                Some(window_proc),
                (&mut *state as *mut State<'_>).cast(),
            );
            command(window.0, &mut state, EXPAND);
            command(window.0, &mut state, NESTED_GROUP_BASE);
            let part = if host_only { 0 } else { 4 };
            unsafe {
                SendMessageW(state.nested[0].chips[part], BM_CLICK, 0, 0);
            }
            assert!(if host_only {
                state.selection.nested[0].host
            } else {
                state.selection.nested[0].paths[3]
            });
            assert!(state.selection.nested[0].queries.iter().all(|on| !on));
            assert!(common::window_text(state.regex).contains("url 内"));
            command(window.0, &mut state, SAVE);
            assert!(!state.expanded);
            assert_eq!(state.action, Action::Back);
            assert!(common::window_text(state.status).contains("保存しました"));
            assert_ne!(unsafe { IsWindow(window.0) }, 0); // 保存は起動・終了しない。
            drop(state);
            drop(save);
            assert_eq!(saved.len(), 1);
            assert!(saved[0].matches_teams_target(&input));
            let encoded = saved[0].encode().unwrap();
            assert!(!encoded.contains("TeamsCID") && !encoded.contains("private"));
        }
    }

    #[test]
    fn nested_chips_save_only_selected_account_and_collapse_without_opening() {
        let mut url = url::Url::parse("https://teams.public.onecdn.static.microsoft/evergreen-assets/safelinks/2/atp-safelinks.html").unwrap();
        url.query_pairs_mut().append_pair("url", "https://tenant.sharepoint.com/a.pdf?TeamsCID=11111111-2222-3333-4444-555555555555&other=private")
            .append_pair("pc", &"synthetic-only-".repeat(100));
        let input = ValidatedUrl::parse(url.to_string()).unwrap();
        let mut saved = vec![];
        let mut save = |rule: &StoredRule| {
            saved.push(rule.clone());
            Ok(())
        };
        let mut state = Box::new(State::new(&input, "Edge / 仕事".into(), &mut save));
        let window = common::HiddenTestWindow::new(
            Some(window_proc),
            (&mut *state as *mut State<'_>).cast(),
        );
        assert!(state.nested.iter().all(|group| !group.expanded));
        command(window.0, &mut state, EXPAND);
        command(window.0, &mut state, SAVE);
        assert!(state.expanded); // 外側ホストだけの自動振り分けは保存しない。
        assert!(common::window_text(state.status).contains("url内で1つ以上"));
        command(window.0, &mut state, NESTED_GROUP_BASE);
        assert!(state.nested[0].expanded);
        let child = &state.parts.nested[0].1;
        let cid_part = 1 + child.paths.len();
        let cid_binding = state
            .nested_bindings
            .iter()
            .position(|binding| *binding == (0, cid_part))
            .unwrap();
        unsafe {
            SendMessageW(state.nested[0].chips[0], BM_CLICK, 0, 0);
        }
        command(window.0, &mut state, NESTED_CHIP_BASE + cid_binding as i32);
        assert!(state.selection.nested[0].host && state.selection.nested[0].queries[0]);
        assert!(common::window_text(state.regex).contains("url 内"));
        unsafe {
            MoveWindow(window.0, 0, 0, 380, 400, 0);
        }
        layout(window.0, &mut state);
        assert!(
            state
                .page
                .controls
                .iter()
                .all(|(_, b)| b.x >= 0 && b.width > 0)
        );
        assert!(
            common::window_text(*state.chips.last().unwrap())
                .chars()
                .count()
                < 160
        );
        command(window.0, &mut state, SAVE);
        assert!(!state.expanded);
        assert_eq!(state.action, Action::Back);
        assert_ne!(unsafe { IsWindow(window.0) }, 0);
        drop(state);
        drop(save);
        assert_eq!(saved.len(), 1);
        assert!(saved[0].matches_teams_target(&input));
        let encoded = saved[0].encode().unwrap();
        assert!(!encoded.contains("synthetic-only") && !encoded.contains("private"));
    }
    const URL: &str = "https://apps.powerapps.com/play/e/default-11111111-2222-3333-4444-555555555555/a/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee?tenantId=11111111-2222-3333-4444-555555555555&hint=66666666-7777-8888-9999-000000000000&source=portal&screen=Requests&itemId=12345&locale=ja-JP&theme=light&mode=edit&hideNavBar=true&debug=false";

    #[test]
    fn chip_uses_common_button_frame_and_keeps_native_checked_state() {
        let mut save = |_: &StoredRule| Ok(());
        let mut state = Box::new(State::new(
            &ValidatedUrl::parse(URL).unwrap(),
            "Chrome / 個人".into(),
            &mut save,
        ));
        let _window = common::HiddenTestWindow::new(
            Some(window_proc),
            (&mut *state as *mut State<'_>).cast(),
        );
        let chip = state.chips[0];
        unsafe {
            MoveWindow(chip, 0, 0, 160, 48, 0);
            let dc = CreateCompatibleDC(null_mut());
            let mut info: BITMAPINFO = std::mem::zeroed();
            info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
            info.bmiHeader.biWidth = 160;
            info.bmiHeader.biHeight = -48;
            info.bmiHeader.biPlanes = 1;
            info.bmiHeader.biBitCount = 32;
            let mut bits = null_mut();
            let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            assert!(!bitmap.is_null());
            let previous = SelectObject(dc, bitmap);
            let mut reference: DRAWITEMSTRUCT = std::mem::zeroed();
            reference.hwndItem = state.back;
            reference.hDC = dc;
            reference.rcItem = RECT {
                left: 0,
                top: 0,
                right: 160,
                bottom: 48,
            };
            // 最初はホストON。クリックでOFFにし、標準ボタンと同じ枠・背景になる。
            SendMessageW(chip, BM_CLICK, 0, 0);
            assert!(!state.selection.host);
            assert_eq!(
                SendMessageW(chip, BM_GETCHECK, 0, 0),
                BST_UNCHECKED as isize
            );
            reference.itemState = if GetFocus() == chip { ODS_FOCUS } else { 0 };
            common::draw_button(&reference);
            let frame = [GetPixel(dc, 0, 0), GetPixel(dc, 1, 24), GetPixel(dc, 14, 8)];
            SendMessageW(chip, WM_PRINTCLIENT, dc as usize, 0);
            assert_eq!(
                [GetPixel(dc, 0, 0), GetPixel(dc, 1, 24), GetPixel(dc, 14, 8)],
                frame
            );
            SendMessageW(chip, BM_CLICK, 0, 0);
            assert!(state.selection.host);
            assert_eq!(SendMessageW(chip, BM_GETCHECK, 0, 0), BST_CHECKED as isize);
            SendMessageW(chip, WM_PRINTCLIENT, dc as usize, 0);
            assert_eq!(GetPixel(dc, 14, 8), common::ACCENT);
            SelectObject(dc, previous);
            DeleteObject(bitmap);
            DeleteDC(dc);
        }
    }

    #[test]
    fn incoming_url_does_not_close_or_overwrite_editor_and_second_pending_is_rejected() {
        use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
        let mut save = |_: &StoredRule| Ok(());
        let mut state = Box::new(State::new(
            &ValidatedUrl::parse(URL).unwrap(),
            "Chrome / 個人".into(),
            &mut save,
        ));
        let window = common::HiddenTestWindow::new(
            Some(window_proc),
            (&mut *state as *mut State<'_>).cast(),
        );
        command(window.0, &mut state, EXPAND);
        let first_query = CHIP_BASE + 1 + state.parts.paths.len() as i32;
        command(window.0, &mut state, first_query);
        let mut encoded = common::wide("https://second.example/path?x=1");
        let data = COPYDATASTRUCT {
            dwData: 1,
            cbData: (encoded.len() * 2) as u32,
            lpData: encoded.as_mut_ptr().cast(),
        };
        let receive =
            || unsafe { SendMessageW(window.0, WM_COPYDATA, 0, &data as *const _ as isize) };
        assert_eq!(receive(), 1);
        assert_eq!(receive(), 0);
        assert!(state.expanded && state.selection.queries[0]);
        assert_ne!(unsafe { IsWindow(window.0) }, 0);
        assert_eq!(state.original_url, URL);
        assert_eq!(
            state.pending_url.as_ref().unwrap().original(),
            "https://second.example/path?x=1"
        );
    }

    #[test]
    fn saving_collapses_without_opening_and_back_preserves_saved_rule() {
        let mut saved = vec![];
        let mut save = |rule: &StoredRule| {
            saved.push(rule.clone());
            Ok(())
        };
        let mut state = Box::new(State::new(
            &ValidatedUrl::parse(URL).unwrap(),
            "Chrome / 個人".into(),
            &mut save,
        ));
        let window = common::HiddenTestWindow::new(
            Some(window_proc),
            (&mut *state as *mut State<'_>).cast(),
        );
        assert!(!state.expanded);
        assert!(state.selection.host);
        assert!(state.selection.queries.iter().all(|on| !*on));
        assert_eq!(
            common::window_text(state.title),
            "以下のURLをChrome / 個人 で開く"
        );
        assert_eq!(common::window_text(state.open), "開く");
        assert_eq!(common::window_text(state.back), "戻る");
        let mut font: LOGFONTW = unsafe { std::mem::zeroed() };
        assert_ne!(
            unsafe {
                GetObjectW(
                    state.code_font,
                    size_of::<LOGFONTW>() as i32,
                    (&mut font as *mut LOGFONTW).cast(),
                )
            },
            0
        );
        let end = font.lfFaceName.iter().position(|ch| *ch == 0).unwrap();
        assert_eq!(
            String::from_utf16_lossy(&font.lfFaceName[..end]),
            "Consolas"
        );
        command(window.0, &mut state, EXPAND);
        let first_query = CHIP_BASE + 1 + state.parts.paths.len() as i32;
        command(window.0, &mut state, first_query);
        command(window.0, &mut state, SAVE);
        assert!(!state.expanded);
        assert_eq!(state.action, Action::Back);
        assert_eq!(common::window_text(state.status), "保存しました。");
        assert!(state.selection.queries[0]);
        command(window.0, &mut state, BACK);
        drop(state);
        assert_eq!(saved.len(), 1);
        assert!(saved[0].regexes[1].contains("tenantId="));
    }

    #[test]
    fn failed_save_keeps_editor_open_and_does_not_open_url() {
        let mut save = |_: &StoredRule| anyhow::bail!("書込み試験");
        let mut state = Box::new(State::new(
            &ValidatedUrl::parse(URL).unwrap(),
            "Chrome / 個人".into(),
            &mut save,
        ));
        let window = common::HiddenTestWindow::new(
            Some(window_proc),
            (&mut *state as *mut State<'_>).cast(),
        );
        command(window.0, &mut state, EXPAND);
        command(window.0, &mut state, SAVE);
        assert!(state.expanded);
        assert_eq!(state.action, Action::Back);
        assert!(common::window_text(state.status).contains("書込み試験"));
    }

    #[test]
    fn only_open_button_returns_open_and_many_parts_fit_at_narrow_width() {
        let mut save = |_: &StoredRule| Ok(());
        let mut state = Box::new(State::new(
            &ValidatedUrl::parse(URL).unwrap(),
            "Chrome / 個人".into(),
            &mut save,
        ));
        let window = common::HiddenTestWindow::new(
            Some(window_proc),
            (&mut *state as *mut State<'_>).cast(),
        );
        command(window.0, &mut state, EXPAND);
        for dpi in [96, 144, 192] {
            state.theme = Theme::new(dpi);
            for width in [360, 520, 760] {
                unsafe {
                    SetWindowPos(
                        window.0,
                        null_mut(),
                        0,
                        0,
                        common::scale(width, dpi),
                        600,
                        SWP_NOMOVE | SWP_NOZORDER,
                    )
                };
                layout(window.0, &mut state);
                let actual = Page::width(window.0, dpi);
                for (_, bounds) in &state.page.controls {
                    assert!(
                        bounds.x >= 0 && bounds.x + bounds.width <= actual,
                        "{dpi}/{width}: {bounds:?}"
                    );
                }
                for (i, (a, ra)) in state.page.controls.iter().enumerate() {
                    for (b, rb) in &state.page.controls[i + 1..] {
                        assert!(
                            a == b
                                || ra.x >= rb.x + rb.width
                                || rb.x >= ra.x + ra.width
                                || ra.y >= rb.y + rb.height
                                || rb.y >= ra.y + ra.height,
                            "overlap {ra:?} {rb:?}"
                        );
                    }
                }
            }
        }
        command(window.0, &mut state, OPEN);
        assert_eq!(state.action, Action::Open);
    }
}
