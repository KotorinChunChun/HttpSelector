//! Win32画面共通の配色、DPI変換、スクロール配置。
use std::mem::size_of;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemServices::{SS_LEFT, SS_NOPREFIX};
use windows_sys::Win32::UI::Controls::{
    DRAWITEMSTRUCT, EM_SETCUEBANNER, EM_SETMARGINS, EM_SETSEL, ODS_DISABLED, ODS_FOCUS,
    ODS_SELECTED, SetScrollInfo, WC_BUTTONW,
};
use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetFocus, GetKeyState, VK_CONTROL};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub const BACKGROUND: u32 = 0x00FAF7F4;
pub const TEXT: u32 = 0x00433523;
pub const MUTED: u32 = 0x0076695B;
pub const ACCENT: u32 = 0x00A66620;
pub const BORDER: u32 = 0x00DFD8CF;

pub fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn is_maximize_command(wparam: usize) -> bool {
    (wparam & 0xfff0) == SC_MAXIMIZE as usize
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Bounds {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
    pub fn bottom(self) -> i32 {
        self.y + self.height
    }
}

pub struct Theme {
    pub dpi: u32,
    pub font: HFONT,
    pub bold: HFONT,
    pub title: HFONT,
    pub background: HBRUSH,
    pub surface: HBRUSH,
}

impl Theme {
    pub fn new(dpi: u32) -> Self {
        let face = wide("Meiryo UI");
        let font = |height, weight| unsafe {
            CreateFontW(
                -scale(height, dpi),
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                0,
                0,
                CLEARTYPE_QUALITY as u32,
                0,
                face.as_ptr(),
            )
        };
        Self {
            dpi,
            font: font(14, 400),
            bold: font(14, 700),
            title: font(23, 700),
            background: unsafe { CreateSolidBrush(BACKGROUND) },
            surface: unsafe { CreateSolidBrush(0x00FFFFFF) },
        }
    }
}

impl Drop for Theme {
    fn drop(&mut self) {
        unsafe {
            for object in [
                self.font,
                self.bold,
                self.title,
                self.background,
                self.surface,
            ] {
                DeleteObject(object);
            }
        }
    }
}

pub fn scale(value: i32, dpi: u32) -> i32 {
    ((value as i64 * dpi as i64 + 48) / 96) as i32
}

pub fn button_text_width(window: HWND, text: &str, font: HFONT, dpi: u32) -> i32 {
    let text = wide(text);
    unsafe {
        let dc = GetDC(window);
        if dc.is_null() {
            return 180;
        }
        let previous = SelectObject(dc, font);
        let mut size = std::mem::zeroed();
        let measured = GetTextExtentPoint32W(dc, text.as_ptr(), text.len() as i32 - 1, &mut size);
        SelectObject(dc, previous);
        ReleaseDC(window, dc);
        if measured == 0 {
            return 180;
        }
        ((size.cx * 96 + dpi as i32 - 1) / dpi as i32) + 24
    }
}

pub struct Tooltips {
    window: HWND,
    texts: Vec<Vec<u16>>,
    controls: Vec<HWND>,
}

impl Tooltips {
    pub fn new(owner: HWND, dpi: u32) -> Self {
        use windows_sys::Win32::UI::Controls::*;
        let controls = INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_WIN95_CLASSES,
        };
        unsafe {
            InitCommonControlsEx(&controls);
        }
        let window = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST,
                TOOLTIPS_CLASSW,
                null(),
                WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                owner,
                null_mut(),
                GetModuleHandleW(null()),
                null(),
            )
        };
        if !window.is_null() {
            unsafe {
                SendMessageW(window, TTM_SETMAXTIPWIDTH, 0, scale(420, dpi) as isize);
                SendMessageW(window, TTM_SETDELAYTIME, TTDT_INITIAL as usize, 350);
                SendMessageW(window, TTM_SETDELAYTIME, TTDT_AUTOPOP as usize, 20000);
            }
        }
        Self {
            window,
            texts: Vec::new(),
            controls: Vec::new(),
        }
    }

    pub fn add(&mut self, owner: HWND, control: HWND, text: &str) {
        use windows_sys::Win32::UI::Controls::*;
        if self.window.is_null() || control.is_null() {
            return;
        }
        let mut text = wide(text);
        let mut info: TTTOOLINFOW = unsafe { std::mem::zeroed() };
        // v6の有効化コンテキストがないWin32画面でも使えるV2サイズを渡す。
        info.cbSize = std::mem::offset_of!(TTTOOLINFOW, lpReserved) as u32;
        info.uFlags = TTF_IDISHWND | TTF_SUBCLASS;
        info.hwnd = owner;
        info.uId = control as usize;
        info.lpszText = text.as_mut_ptr();
        if unsafe {
            SendMessageW(
                self.window,
                TTM_ADDTOOLW,
                0,
                (&info as *const TTTOOLINFOW) as isize,
            )
        } != 0
        {
            self.texts.push(text);
            self.controls.push(control);
        }
    }

    #[cfg(test)]
    pub fn count(&self) -> usize {
        unsafe {
            SendMessageW(
                self.window,
                windows_sys::Win32::UI::Controls::TTM_GETTOOLCOUNT,
                0,
                0,
            ) as usize
        }
    }

    #[cfg(test)]
    pub fn contains(&self, control: HWND) -> bool {
        self.controls.contains(&control)
    }
}

impl Drop for Tooltips {
    fn drop(&mut self) {
        if !self.window.is_null() && unsafe { IsWindow(self.window) } != 0 {
            unsafe {
                DestroyWindow(self.window);
            }
        }
    }
}

pub fn initial_bounds(width: i32, height: i32, style: u32, ex_style: u32) -> Bounds {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let mut cursor = POINT { x: 0, y: 0 };
        GetCursorPos(&mut cursor);
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return Bounds::new(CW_USEDEFAULT, CW_USEDEFAULT, width, height);
        }
        let mut dpi_x = 96;
        let mut dpi_y = 96;
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        let mut frame = RECT {
            left: 0,
            top: 0,
            right: scale(width, dpi_x),
            bottom: scale(height, dpi_x),
        };
        AdjustWindowRectExForDpi(&mut frame, style, 0, ex_style, dpi_x);
        let work = info.rcWork;
        let width = (frame.right - frame.left).min(work.right - work.left);
        let height = (frame.bottom - frame.top).min(work.bottom - work.top);
        Bounds::new(
            work.left + (work.right - work.left - width) / 2,
            work.top + (work.bottom - work.top - height) / 2,
            width,
            height,
        )
    }
}

pub fn control(parent: HWND, class: &str, text: &str, id: i32, style: u32, font: HFONT) -> HWND {
    unsafe {
        let window = CreateWindowExW(
            0,
            wide(class).as_ptr(),
            wide(text).as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            0,
            0,
            1,
            1,
            parent,
            id as usize as _,
            GetModuleHandleW(null()),
            null_mut(),
        );
        SendMessageW(window, WM_SETFONT, font as usize, 0);
        window
    }
}

pub fn label(parent: HWND, text: &str, font: HFONT) -> HWND {
    control(parent, "STATIC", text, 0, SS_LEFT | SS_NOPREFIX, font)
}

pub fn button(parent: HWND, text: &str, id: i32, font: HFONT) -> HWND {
    control(
        parent,
        "BUTTON",
        text,
        id,
        WS_TABSTOP | BS_PUSHBUTTON as u32 | BS_OWNERDRAW as u32,
        font,
    )
}

pub fn edit(parent: HWND, id: i32, multiline: bool, readonly: bool, font: HFONT) -> HWND {
    let mut style = WS_TABSTOP | WS_BORDER;
    style |= if multiline {
        WS_VSCROLL | ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32
    } else {
        ES_AUTOHSCROLL as u32
    };
    if readonly {
        style |= ES_READONLY as u32;
    }
    let window = control(parent, "EDIT", "", id, style, font);
    unsafe {
        SendMessageW(
            window,
            EM_SETMARGINS,
            (EC_LEFTMARGIN | EC_RIGHTMARGIN) as usize,
            8 | (8 << 16),
        );
    }
    window
}

pub fn cue(window: HWND, value: &str) {
    unsafe {
        SendMessageW(window, EM_SETCUEBANNER, 1, wide(value).as_ptr() as isize);
    }
}

pub fn set_text(window: HWND, value: &str) {
    unsafe {
        SetWindowTextW(window, wide(value).as_ptr());
    }
}

/// 外部アプリや補助画面を表示する前に、対象ウィンドウの最前面属性を解除する。
pub fn release_topmost(window: HWND) {
    if window.is_null() {
        return;
    }
    unsafe {
        SetWindowPos(
            window,
            HWND_NOTOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

pub fn window_text(window: HWND) -> String {
    unsafe {
        let length = GetWindowTextLengthW(window);
        let mut buffer = vec![0_u16; length as usize + 1];
        let copied = GetWindowTextW(window, buffer.as_mut_ptr(), buffer.len() as i32);
        String::from_utf16_lossy(&buffer[..copied as usize])
    }
}

#[derive(Default)]
pub struct Page {
    pub controls: Vec<(HWND, Bounds)>,
    pub offset: i32,
    pub height: i32,
    pub viewport: i32,
    wheel_remainder: i32,
}

impl Page {
    pub fn width(window: HWND, dpi: u32) -> i32 {
        let mut rect: RECT = unsafe { std::mem::zeroed() };
        unsafe {
            GetClientRect(window, &mut rect);
        }
        rect.right * 96 / dpi.max(1) as i32
    }

    pub fn place(&mut self, window: HWND, bounds: Bounds) {
        self.controls.push((window, bounds));
    }

    pub fn apply(&mut self, window: HWND, dpi: u32) {
        unsafe {
            let mut rect: RECT = std::mem::zeroed();
            GetClientRect(window, &mut rect);
            self.viewport = rect.bottom;
            self.offset = self
                .offset
                .clamp(0, (scale(self.height, dpi) - self.viewport).max(0));
            let info = SCROLLINFO {
                cbSize: size_of::<SCROLLINFO>() as u32,
                fMask: SIF_RANGE | SIF_PAGE | SIF_POS | SIF_DISABLENOSCROLL,
                nMin: 0,
                nMax: scale(self.height, dpi) - 1,
                nPage: self.viewport as u32,
                nPos: self.offset,
                nTrackPos: 0,
            };
            SetScrollInfo(window, SB_VERT, &info, 1);
            for (child, bounds) in &self.controls {
                SetWindowPos(
                    *child,
                    null_mut(),
                    scale(bounds.x, dpi),
                    scale(bounds.y, dpi) - self.offset,
                    scale(bounds.width, dpi),
                    scale(bounds.height, dpi),
                    SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOREDRAW | SWP_NOCOPYBITS,
                );
            }
            // 移動前の画素をコピーしない。兄弟コントロールの旧描画が残らないよう一括再描画する。
            RedrawWindow(
                window,
                null(),
                null_mut(),
                RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN,
            );
        }
    }

    pub fn scroll(&mut self, window: HWND, message: u32, wparam: usize, dpi: u32) {
        if message == WM_MOUSEWHEEL {
            self.wheel_remainder += ((wparam >> 16) as u16 as i16) as i32;
            let steps = self.wheel_remainder / WHEEL_DELTA as i32;
            self.wheel_remainder %= WHEEL_DELTA as i32;
            self.offset -= steps * scale(72, dpi);
        } else {
            let code = (wparam & 0xffff) as i32;
            match code {
                SB_LINEUP => self.offset -= scale(32, dpi),
                SB_LINEDOWN => self.offset += scale(32, dpi),
                SB_PAGEUP => self.offset -= self.viewport,
                SB_PAGEDOWN => self.offset += self.viewport,
                SB_TOP => self.offset = 0,
                SB_BOTTOM => self.offset = scale(self.height, dpi),
                SB_THUMBTRACK | SB_THUMBPOSITION => unsafe {
                    let mut info: SCROLLINFO = std::mem::zeroed();
                    info.cbSize = size_of::<SCROLLINFO>() as u32;
                    info.fMask = SIF_TRACKPOS;
                    GetScrollInfo(window, SB_VERT, &mut info);
                    self.offset = info.nTrackPos;
                },
                _ => return,
            }
        }
        self.apply(window, dpi);
    }

    pub fn reveal(&mut self, window: HWND, focus: HWND, dpi: u32) {
        if let Some((_, bounds)) = self.controls.iter().find(|(child, _)| *child == focus) {
            let mut top = scale(bounds.y, dpi);
            let mut bottom = scale(bounds.bottom(), dpi);
            // 一覧全体が画面より高い場合も、キーボードで選択中の行を見せる。
            let mut class = [0_u16; 32];
            let length = unsafe { GetClassNameW(focus, class.as_mut_ptr(), class.len() as i32) };
            let class = String::from_utf16_lossy(&class[..length as usize]);
            if class.eq_ignore_ascii_case("ComboBox") {
                // 配置用の高さには展開一覧を含むが、スクロールで見せるのは閉じた入力欄。
                let mut rect: RECT = unsafe { std::mem::zeroed() };
                unsafe { GetWindowRect(focus, &mut rect) };
                bottom = top + rect.bottom - rect.top;
            } else if class.eq_ignore_ascii_case("ListBox") {
                unsafe {
                    let selected = SendMessageW(focus, LB_GETCURSEL, 0, 0);
                    let mut row: RECT = std::mem::zeroed();
                    if selected != LB_ERR as isize
                        && SendMessageW(
                            focus,
                            LB_GETITEMRECT,
                            selected as usize,
                            &mut row as *mut RECT as isize,
                        ) != LB_ERR as isize
                    {
                        bottom = top + row.bottom;
                        top += row.top;
                    }
                }
            }
            let previous = self.offset;
            if top < self.offset {
                self.offset = top;
            } else if bottom > self.offset + self.viewport {
                self.offset = bottom - self.viewport;
            }
            if previous != self.offset {
                self.apply(window, dpi);
            }
        }
    }
}

pub fn update_fonts(page: &Page, old: &Theme, new: &Theme) {
    for (child, _) in &page.controls {
        unsafe {
            let previous = SendMessageW(*child, WM_GETFONT, 0, 0) as HFONT;
            let font = if previous == old.title {
                new.title
            } else if previous == old.bold {
                new.bold
            } else {
                new.font
            };
            SendMessageW(*child, WM_SETFONT, font as usize, 1);
        }
    }
}

pub fn draw_text(dc: HDC, text: &str, mut rect: RECT, font: HFONT, color: u32) {
    let text = wide(&text.split_whitespace().collect::<Vec<_>>().join(" "));
    unsafe {
        let previous = SelectObject(dc, font);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, color);
        DrawTextW(
            dc,
            text.as_ptr(),
            (text.len() - 1) as i32,
            &mut rect,
            DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        SelectObject(dc, previous);
    }
}

/// 共通の操作ボタンを、アプリの配色に合わせて描画する。
pub fn draw_button(item: &DRAWITEMSTRUCT) {
    draw_button_state(item, false, false);
}

/// 操作ボタンとURL部分トグルで、枠線・角丸・配色を共有する。
pub fn draw_button_state(item: &DRAWITEMSTRUCT, checked: bool, multiline: bool) {
    let rect = item.rcItem;
    let disabled = item.itemState & ODS_DISABLED != 0;
    let selected = checked || item.itemState & ODS_SELECTED != 0;
    let focused = item.itemState & ODS_FOCUS != 0;
    let (fill, border, text) = if disabled {
        (0x00F1EFEC, 0x00D8D3CD, MUTED)
    } else if selected {
        (ACCENT, ACCENT, 0x00FFFFFF)
    } else if focused {
        (0x00F7EBDD, ACCENT, TEXT)
    } else {
        (0x00FFFFFF, 0x00CDBDAA, TEXT)
    };
    unsafe {
        let background = CreateSolidBrush(BACKGROUND);
        FillRect(item.hDC, &rect, background);
        DeleteObject(background);

        let fill_brush = CreateSolidBrush(fill);
        let border_pen = CreatePen(PS_SOLID, 1, border);
        let old_brush = SelectObject(item.hDC, fill_brush);
        let old_pen = SelectObject(item.hDC, border_pen);
        RoundRect(
            item.hDC,
            rect.left + 1,
            rect.top + 1,
            rect.right - 1,
            rect.bottom - 1,
            10,
            10,
        );
        SelectObject(item.hDC, old_brush);
        SelectObject(item.hDC, old_pen);
        DeleteObject(fill_brush);
        DeleteObject(border_pen);

        let font = SendMessageW(item.hwndItem, WM_GETFONT, 0, 0) as HFONT;
        let old_font = if !font.is_null() {
            SelectObject(item.hDC, font)
        } else {
            null_mut()
        };
        SetBkMode(item.hDC, TRANSPARENT as i32);
        SetTextColor(item.hDC, text);
        let mut text_rect = RECT {
            left: rect.left + 10,
            top: rect.top + 2,
            right: rect.right - 10,
            bottom: rect.bottom - 2,
        };
        let value = wide(&window_text(item.hwndItem));
        let flags = if multiline {
            let mut measured = text_rect;
            DrawTextW(
                item.hDC,
                value.as_ptr(),
                (value.len() - 1) as i32,
                &mut measured,
                DT_CALCRECT | DT_CENTER | DT_WORDBREAK | DT_NOPREFIX,
            );
            text_rect.top +=
                ((text_rect.bottom - text_rect.top) - (measured.bottom - measured.top)).max(0) / 2;
            DT_CENTER | DT_WORDBREAK | DT_NOPREFIX
        } else {
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX
        };
        DrawTextW(
            item.hDC,
            value.as_ptr(),
            (value.len() - 1) as i32,
            &mut text_rect,
            flags,
        );
        if !font.is_null() {
            SelectObject(item.hDC, old_font);
        }
        if focused && !disabled {
            let focus_rect = RECT {
                left: rect.left + 5,
                top: rect.top + 5,
                right: rect.right - 5,
                bottom: rect.bottom - 5,
            };
            DrawFocusRect(item.hDC, &focus_rect);
        }
    }
}

pub fn is_button(window: HWND) -> bool {
    let mut class = [0_u16; 32];
    unsafe {
        GetClassNameW(window, class.as_mut_ptr(), class.len() as i32);
    }
    &class[..6] == unsafe { std::slice::from_raw_parts(WC_BUTTONW, 6) }
}

/// 標準EDITでは自動提供されないCtrl+Aを、URL・設定入力欄へ共通で提供する。
pub fn handle_edit_shortcut(message: &MSG) -> bool {
    if message.message != WM_KEYDOWN
        || message.wParam != b'A' as usize
        || unsafe { GetKeyState(VK_CONTROL as i32) } >= 0
    {
        return false;
    }
    let focus = unsafe { GetFocus() };
    let mut class = [0_u16; 32];
    let length = unsafe { GetClassNameW(focus, class.as_mut_ptr(), class.len() as i32) };
    if !String::from_utf16_lossy(&class[..length as usize]).eq_ignore_ascii_case("Edit") {
        return false;
    }
    unsafe {
        SendMessageW(focus, EM_SETSEL, 0, -1);
    }
    true
}

/// 実データ・外部プロセス・表示中のアプリに触れないWin32回帰テスト用。
#[cfg(test)]
pub(crate) struct HiddenTestWindow(pub HWND);

#[cfg(test)]
impl HiddenTestWindow {
    pub fn new(proc: WNDPROC, state: *mut std::ffi::c_void) -> Self {
        unsafe {
            let class = wide(&format!(
                "HttpSelector.Test.{:?}",
                std::thread::current().id()
            ));
            let mut descriptor: WNDCLASSW = std::mem::zeroed();
            descriptor.lpfnWndProc = proc;
            descriptor.hInstance = GetModuleHandleW(null());
            descriptor.lpszClassName = class.as_ptr();
            RegisterClassW(&descriptor);
            let window = CreateWindowExW(
                0,
                class.as_ptr(),
                wide("test").as_ptr(),
                WS_OVERLAPPED | WS_CLIPCHILDREN,
                0,
                0,
                940,
                800,
                null_mut(),
                null_mut(),
                descriptor.hInstance,
                state,
            );
            assert!(!window.is_null());
            Self(window)
        }
    }
}

#[cfg(test)]
impl Drop for HiddenTestWindow {
    fn drop(&mut self) {
        unsafe {
            if IsWindow(self.0) != 0 {
                DestroyWindow(self.0);
            }
            let mut message = std::mem::zeroed();
            while PeekMessageW(&mut message, null_mut(), WM_QUIT, WM_QUIT, PM_REMOVE) != 0 {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_maximize_system_command_is_rejected() {
        assert!(is_maximize_command(SC_MAXIMIZE as usize));
        assert!(is_maximize_command(SC_MAXIMIZE as usize | 3));
        assert!(!is_maximize_command(SC_RESTORE as usize));
        assert!(!is_maximize_command(SC_MINIMIZE as usize));
    }

    #[test]
    fn japanese_font_fits_card_lines_at_each_dpi() {
        for dpi in [96, 120, 144, 192] {
            let theme = Theme::new(dpi);
            unsafe {
                let dc = CreateCompatibleDC(null_mut());
                assert!(!dc.is_null());
                for (font, height) in [(theme.font, 22), (theme.bold, 24), (theme.title, 32)] {
                    assert!(!font.is_null());
                    let previous = SelectObject(dc, font);
                    let mut metrics: TEXTMETRICW = std::mem::zeroed();
                    assert_ne!(GetTextMetricsW(dc, &mut metrics), 0);
                    assert!(
                        metrics.tmHeight <= scale(height, dpi),
                        "DPI {dpi}: 文字高 {} が領域を超える",
                        metrics.tmHeight
                    );
                    SelectObject(dc, previous);
                }
                DeleteDC(dc);
            }
        }
    }
}
