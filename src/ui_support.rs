//! Win32画面共通の配色、DPI変換、スクロール配置。
use std::mem::size_of;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemServices::{SS_LEFT, SS_NOPREFIX};
use windows_sys::Win32::UI::Controls::{EM_SETCUEBANNER, EM_SETMARGINS, SetScrollInfo, WC_BUTTONW};
use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub const BACKGROUND: u32 = 0x00FAF7F4;
pub const TEXT: u32 = 0x00433523;
pub const MUTED: u32 = 0x0076695B;
pub const ACCENT: u32 = 0x00A66620;
pub const BORDER: u32 = 0x00DFD8CF;

pub fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
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
        WS_TABSTOP | BS_PUSHBUTTON as u32,
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
                MoveWindow(
                    *child,
                    scale(bounds.x, dpi),
                    scale(bounds.y, dpi) - self.offset,
                    scale(bounds.width, dpi),
                    scale(bounds.height, dpi),
                    1,
                );
            }
            InvalidateRect(window, null(), 1);
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
            let top = scale(bounds.y, dpi);
            let bottom = scale(bounds.bottom(), dpi);
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

pub fn is_button(window: HWND) -> bool {
    let mut class = [0_u16; 32];
    unsafe {
        GetClassNameW(window, class.as_mut_ptr(), class.len() as i32);
    }
    &class[..6] == unsafe { std::slice::from_raw_parts(WC_BUTTONW, 6) }
}
