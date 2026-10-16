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
        Input::KeyboardAndMouse::{GetFocus, SetFocus, VK_ESCAPE, VK_RETURN},
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
const CHIP_BASE: i32 = 1000;

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
    action: Action,
    pending_url: Option<ValidatedUrl>,
    save: &'a mut dyn FnMut(&StoredRule) -> anyhow::Result<()>,
    theme: Theme,
    code_font: HFONT,
    page: Page,
    fixed: Vec<HWND>,
    chips: Vec<HWND>,
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
    status: HWND,
}

impl<'a> State<'a> {
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
            action: Action::Back,
            pending_url: None,
            save,
            theme: Theme::new(96),
            code_font: null_mut(),
            page: Page::default(),
            fixed: vec![],
            chips: vec![],
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
            status: null_mut(),
        }
    }
    fn rule(&self) -> anyhow::Result<StoredRule> {
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
    state.expand = common::button(window, "▶ 今後の動作を設定", EXPAND, font);
    state.scheme = common::label(window, "http(s)://", font);
    let labels = std::iter::once(state.parts.host.clone())
        .chain(state.parts.paths.iter().map(|p| format!("/ {p}")))
        .chain(state.parts.queries.iter().cloned())
        .collect::<Vec<_>>();
    for (index, label) in labels.iter().enumerate() {
        state.chips.push(common::control(
            window,
            "BUTTON",
            label,
            CHIP_BASE + index as i32,
            WS_TABSTOP | BS_AUTOCHECKBOX as u32 | BS_PUSHLIKE as u32 | BS_MULTILINE as u32,
            font,
        ));
    }
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
    ];
    update(state);
    layout(window, state);
}

fn update(state: &State<'_>) {
    let flags = std::iter::once(state.selection.host)
        .chain(state.selection.paths.iter().copied())
        .chain(state.selection.queries.iter().copied());
    for (control, on) in state.chips.iter().zip(flags) {
        unsafe {
            SendMessageW(
                *control,
                BM_SETCHECK,
                if on { BST_CHECKED } else { BST_UNCHECKED } as usize,
                0,
            )
        };
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
    match state.rule() {
        Ok(rule) => {
            common::set_text(state.regex, &rule.regexes.join("\r\n\r\n"));
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
}

fn command(window: HWND, state: &mut State<'_>, id: i32) {
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
        SAVE => {
            let result = state.rule().and_then(|rule| (state.save)(&rule));
            match result {
                Ok(()) => {
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
                        &state.parts.queries[row_index * columns as usize + index],
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
    const URL: &str = "https://apps.powerapps.com/play/e/default-11111111-2222-3333-4444-555555555555/a/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee?tenantId=11111111-2222-3333-4444-555555555555&hint=66666666-7777-8888-9999-000000000000&source=portal&screen=Requests&itemId=12345&locale=ja-JP&theme=light&mode=edit&hideNavBar=true&debug=false";

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
