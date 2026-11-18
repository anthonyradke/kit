//! Battery levels on the taskbar, just left of the notification area, read
//! like "🎧 53%  🖱 93%": the device's glyph from the Windows icon font, then
//! its level. Red at 15% or less unless it's charging, a bolt while it
//! charges, dimmed (with the last reading) while the device is asleep or off.
//!
//! Windows has no API for this. The widget is a small window of kit's own
//! made a child of the taskbar (`Shell_TrayWnd`) and drawn with per-pixel
//! alpha, so the taskbar shows through around the text. Being a child of
//! explorer's window ties this thread's input to explorer's, so the thread
//! does nothing but draw: readings arrive through [`update`] and a message.
//! Every two seconds it follows the notification area (which grows as icons
//! come and go) and comes back after explorer restarts. Vertical taskbars
//! get nothing.

use std::sync::{mpsc, Mutex, OnceLock};

use tauri::AppHandle;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, GdiFlush, GetDC, GetTextExtentPoint32W,
    ReleaseDC, ScreenToClient, SelectObject, SetBkMode, SetTextColor, TextOutW, AC_SRC_ALPHA, AC_SRC_OVER,
    ANTIALIASED_QUALITY, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET,
    DEFAULT_PITCH, DIB_RGB_COLORS, FW_NORMAL, HDC, HFONT, OUT_DEFAULT_PRECIS, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowExW, FindWindowW, GetMessageW, GetParent,
    GetWindowRect, IsWindow, LoadCursorW, PeekMessageW, PostThreadMessageW, RegisterClassExW, SetTimer, SetWindowPos,
    TranslateMessage, UpdateLayeredWindow, HWND_TOP, IDC_ARROW, MA_NOACTIVATE, MSG, PM_NOREMOVE, SWP_NOACTIVATE,
    SWP_SHOWWINDOW, ULW_ALPHA, WM_APP, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_TIMER, WM_USER, WNDCLASSEXW, WS_CHILD,
    WS_CLIPSIBLINGS, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Headset,
    Mouse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reading {
    pub kind: Kind,
    /// Percent; `None` while there is none (a headset that's off).
    pub battery: Option<u8>,
    pub charging: bool,
    /// Asleep, off or out of range.
    pub away: bool,
}

/// What the widget draws for one device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Shown {
    kind: Kind,
    percent: u8,
    charging: bool,
    away: bool,
}

/// At or below this, and not charging, the level is red (the tray's mark too).
pub const LOW: u8 = 15;

#[derive(Default)]
struct State {
    on: bool,
    shown: Vec<Shown>,
}

static STATE: Mutex<State> = Mutex::new(State { on: false, shown: Vec::new() });
static THREAD: OnceLock<u32> = OnceLock::new();
static APP: OnceLock<AppHandle> = OnceLock::new();

const WM_REDRAW: u32 = WM_APP + 1;
const FOLLOW_MS: u32 = 2000;

/// What to draw for `readings`, given what was drawn before: a device with no
/// reading right now keeps its last one, dimmed; one that never had any is
/// left out.
fn shown(readings: &[Reading], before: &[Shown]) -> Vec<Shown> {
    readings
        .iter()
        .filter_map(|r| match r.battery {
            Some(percent) => Some(Shown { kind: r.kind, percent: percent.min(100), charging: r.charging && !r.away, away: r.away }),
            None => before.iter().find(|b| b.kind == r.kind).map(|b| Shown { charging: false, away: true, ..*b }),
        })
        .collect()
}

/// Show these readings on the taskbar (`on` false hides the widget). Cheap
/// when nothing changed, so every status refresh can call it.
pub fn update(app: &AppHandle, on: bool, readings: &[Reading]) {
    {
        let mut s = STATE.lock().unwrap();
        let next = shown(readings, &s.shown);
        if s.on == on && s.shown == next && THREAD.get().is_some() {
            return;
        }
        s.on = on;
        s.shown = next;
    }
    let _ = APP.set(app.clone());
    let thread = *THREAD.get_or_init(spawn);
    if thread != 0 {
        unsafe { PostThreadMessageW(thread, WM_REDRAW, 0, 0) };
    }
}

fn spawn() -> u32 {
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new().name("taskbar".into()).spawn(move || unsafe {
        // Make the thread's message queue before anyone posts to it.
        let mut msg: MSG = std::mem::zeroed();
        PeekMessageW(&mut msg, std::ptr::null_mut(), WM_USER, WM_USER, PM_NOREMOVE);
        let _ = tx.send(GetCurrentThreadId());
        run();
    });
    if let Err(e) = spawned {
        log::error!("taskbar: can't start the widget thread: {e}");
        return 0;
    }
    rx.recv().unwrap_or(0)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

unsafe fn run() {
    let class = wide("kitTaskbarBattery");
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(window_proc),
        hInstance: GetModuleHandleW(std::ptr::null()),
        hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
        lpszClassName: class.as_ptr(),
        ..std::mem::zeroed()
    };
    if RegisterClassExW(&wc) == 0 {
        log::error!("taskbar: can't register the widget's window class");
        return;
    }
    // A thread timer: WM_TIMER arrives with no window.
    SetTimer(std::ptr::null_mut(), 0, FOLLOW_MS, None);
    let mut widget = Widget { hwnd: std::ptr::null_mut(), drawn: None };
    let mut msg: MSG = std::mem::zeroed();
    while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
        if msg.hwnd.is_null() && (msg.message == WM_REDRAW || msg.message == WM_TIMER) {
            widget.refresh(class.as_ptr());
            continue;
        }
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_LBUTTONUP => {
            if let Some(app) = APP.get() {
                let handle = app.clone();
                let _ = app.run_on_main_thread(move || crate::show_window(&handle));
            }
            0
        }
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// What the window last showed, so a timer tick that changes nothing costs
/// nothing.
#[derive(PartialEq)]
struct Drawn {
    shown: Vec<Shown>,
    dpi: u32,
    light: bool,
    height: i32,
    width: i32,
    at: (i32, i32),
}

struct Widget {
    hwnd: HWND,
    drawn: Option<Drawn>,
}

impl Widget {
    unsafe fn hide(&mut self) {
        if !self.hwnd.is_null() {
            DestroyWindow(self.hwnd);
            self.hwnd = std::ptr::null_mut();
        }
        self.drawn = None;
    }

    unsafe fn refresh(&mut self, class: *const u16) {
        let (on, shown) = {
            let s = STATE.lock().unwrap();
            (s.on, s.shown.clone())
        };
        let taskbar = FindWindowW(wide("Shell_TrayWnd").as_ptr(), std::ptr::null());
        let mut bar: RECT = std::mem::zeroed();
        if !on || shown.is_empty() || taskbar.is_null() || GetWindowRect(taskbar, &mut bar) == 0 {
            return self.hide();
        }
        let height = bar.bottom - bar.top;
        if bar.right - bar.left < height {
            return self.hide();
        }
        // Explorer restarted, or it's the first time: a new window in the new taskbar.
        if self.hwnd.is_null() || IsWindow(self.hwnd) == 0 || GetParent(self.hwnd) != taskbar {
            self.hide();
            self.hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class,
                wide("kit battery").as_ptr(),
                WS_CHILD | WS_CLIPSIBLINGS,
                0,
                0,
                1,
                1,
                taskbar,
                std::ptr::null_mut(),
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null(),
            );
            if self.hwnd.is_null() {
                log::warn!("taskbar: can't place the battery widget on the taskbar");
                return;
            }
        }
        let dpi = GetDpiForWindow(taskbar).max(96);
        let light = light_taskbar();
        // Right-aligned against the notification area.
        let notify = FindWindowExW(taskbar, std::ptr::null_mut(), wide("TrayNotifyWnd").as_ptr(), std::ptr::null());
        let mut tray: RECT = std::mem::zeroed();
        let right = if !notify.is_null() && GetWindowRect(notify, &mut tray) != 0 { tray.left } else { bar.right - px(240, dpi) };
        let reuse = self.drawn.as_ref().filter(|d| d.shown == shown && d.dpi == dpi && d.light == light && d.height == height);
        let width = match reuse {
            Some(d) => d.width,
            None => draw(self.hwnd, &shown, dpi, light, height),
        };
        let mut at = POINT { x: right - px(6, dpi) - width, y: bar.top };
        ScreenToClient(taskbar, &mut at);
        let next = Drawn { shown, dpi, light, height, width, at: (at.x, at.y) };
        if self.drawn.as_ref() != Some(&next) {
            SetWindowPos(self.hwnd, HWND_TOP, at.x, at.y, width, height, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
        self.drawn = Some(next);
    }
}

fn px(v: i32, dpi: u32) -> i32 {
    (v * dpi as i32 + 48) / 96
}

/// The taskbar follows Windows' mode (not the apps' mode).
fn light_taskbar() -> bool {
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let name = wide("SystemUsesLightTheme");
    let (mut value, mut size) = (0u32, size_of::<u32>() as u32);
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut value as *mut u32).cast(),
            &mut size,
        )
    } == 0;
    ok && value == 1
}

/// sRGB, straight (not premultiplied).
type Rgb = [u8; 3];

struct Palette {
    ink: Rgb,
    low: Rgb,
    bolt: Rgb,
}

/// Windows 11's own text, critical and accent colors on each taskbar.
fn palette(light: bool) -> Palette {
    if light {
        Palette { ink: [0x1A, 0x1A, 0x1A], low: [0xC4, 0x2B, 0x1C], bolt: [0x00, 0x5F, 0xB8] }
    } else {
        Palette { ink: [0xFF, 0xFF, 0xFF], low: [0xFF, 0x99, 0xA4], bolt: [0x60, 0xCD, 0xFF] }
    }
}

/// Segoe Fluent Icons code points.
fn glyph(kind: Kind) -> &'static str {
    match kind {
        Kind::Headset => "\u{E7F6}",
        Kind::Mouse => "\u{E962}",
    }
}
const BOLT: &str = "\u{E945}";

struct Run {
    font: HFONT,
    text: Vec<u16>,
    x: i32,
    color: Rgb,
    opacity: f32,
}

unsafe fn font(face: &str, px_height: i32) -> HFONT {
    let face = wide(face);
    CreateFontW(
        -px_height,
        0,
        0,
        0,
        FW_NORMAL as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET as u32,
        OUT_DEFAULT_PRECIS as u32,
        CLIP_DEFAULT_PRECIS as u32,
        ANTIALIASED_QUALITY as u32,
        DEFAULT_PITCH as u32,
        face.as_ptr(),
    )
}

unsafe fn extent(dc: HDC, font: HFONT, text: &[u16]) -> SIZE {
    let mut size: SIZE = std::mem::zeroed();
    SelectObject(dc, font as _);
    GetTextExtentPoint32W(dc, text.as_ptr(), text.len() as i32, &mut size);
    size
}

/// A top-down 32-bit DIB of `w` x `h` selected into `dc`; returns the bitmap
/// and its pixels (BGRA).
unsafe fn dib(dc: HDC, w: i32, h: i32) -> Option<(windows_sys::Win32::Graphics::Gdi::HBITMAP, &'static mut [u32])> {
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..std::mem::zeroed()
        },
        ..std::mem::zeroed()
    };
    let mut bits = std::ptr::null_mut();
    let bmp = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
    if bmp.is_null() || bits.is_null() {
        return None;
    }
    SelectObject(dc, bmp as _);
    Some((bmp, std::slice::from_raw_parts_mut(bits as *mut u32, (w * h) as usize)))
}

/// Lay the readings out, draw them and hand the picture to the window.
/// Returns the width it took.
unsafe fn draw(hwnd: HWND, shown: &[Shown], dpi: u32, light: bool, height: i32) -> i32 {
    let colors = palette(light);
    let icons = font("Segoe Fluent Icons", px(16, dpi));
    let text = font("Segoe UI", px(15, dpi));
    let bolt = font("Segoe Fluent Icons", px(12, dpi));
    let screen = GetDC(std::ptr::null_mut());
    let measure = CreateCompatibleDC(screen);

    let mut runs = Vec::new();
    let mut x = px(4, dpi);
    for (i, s) in shown.iter().enumerate() {
        if i > 0 {
            x += px(14, dpi);
        }
        let low = s.percent <= LOW && !s.charging;
        let color = if low { colors.low } else { colors.ink };
        let opacity = if s.away { 0.45 } else { 1.0 };
        for (f, t, c, gap) in [
            (icons, glyph(s.kind).to_string(), color, px(5, dpi)),
            (text, format!("{}%", s.percent), color, 0),
        ] {
            let t: Vec<u16> = t.encode_utf16().collect();
            let w = extent(measure, f, &t).cx;
            runs.push(Run { font: f, text: t, x, color: c, opacity });
            x += w + gap;
        }
        if s.charging {
            x += px(2, dpi);
            let t: Vec<u16> = BOLT.encode_utf16().collect();
            let w = extent(measure, bolt, &t).cx;
            runs.push(Run { font: bolt, text: t, x, color: colors.bolt, opacity });
            x += w;
        }
    }
    let width = (x + px(4, dpi)).max(1);

    // Each run is drawn white on black, where GDI's grayscale antialiasing
    // leaves the coverage, then laid onto the picture in its color with that
    // coverage as alpha (premultiplied, as UpdateLayeredWindow wants).
    let out_dc = CreateCompatibleDC(screen);
    let scratch_dc = CreateCompatibleDC(screen);
    if let (Some((out_bmp, out)), Some((scratch_bmp, scratch))) = (dib(out_dc, width, height), dib(scratch_dc, width, height)) {
        out.fill(0);
        SetBkMode(scratch_dc, TRANSPARENT as i32);
        SetTextColor(scratch_dc, 0x00FF_FFFF);
        for run in &runs {
            scratch.fill(0);
            let size = extent(scratch_dc, run.font, &run.text);
            TextOutW(scratch_dc, run.x, (height - size.cy) / 2, run.text.as_ptr(), run.text.len() as i32);
            GdiFlush();
            for (o, s) in out.iter_mut().zip(scratch.iter()) {
                let coverage = ((*s >> 16) & 0xFF).max((*s >> 8) & 0xFF).max(*s & 0xFF);
                if coverage == 0 {
                    continue;
                }
                let a = coverage as f32 / 255.0 * run.opacity;
                let [r, g, b] = run.color.map(|c| c as f32 * a);
                // Over what's there (runs don't overlap, but their edges may touch).
                let keep = 1.0 - a;
                let old = |shift: u32| ((*o >> shift) & 0xFF) as f32 * keep;
                let ch = |v: f32| (v.round().clamp(0.0, 255.0)) as u32;
                *o = ch(a * 255.0 + old(24)) << 24 | ch(r + old(16)) << 16 | ch(g + old(8)) << 8 | ch(b + old(0));
            }
        }
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
        let size = SIZE { cx: width, cy: height };
        let origin = POINT { x: 0, y: 0 };
        UpdateLayeredWindow(hwnd, std::ptr::null_mut(), std::ptr::null(), &size, out_dc, &origin, 0, &blend, ULW_ALPHA);
        DeleteObject(out_bmp as _);
        DeleteObject(scratch_bmp as _);
    }
    DeleteDC(out_dc);
    DeleteDC(scratch_dc);
    DeleteDC(measure);
    ReleaseDC(std::ptr::null_mut(), screen);
    for f in [icons, text, bolt] {
        DeleteObject(f as _);
    }
    width
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(kind: Kind, battery: Option<u8>, away: bool) -> Reading {
        Reading { kind, battery, charging: false, away }
    }

    #[test]
    fn a_device_without_a_reading_keeps_its_last_one_dimmed() {
        let first = shown(&[reading(Kind::Headset, Some(53), false), reading(Kind::Mouse, Some(93), false)], &[]);
        assert_eq!(first.len(), 2);
        // The headset turned off: no reading, so its 53% stays, dimmed.
        let next = shown(&[reading(Kind::Headset, None, true), reading(Kind::Mouse, Some(93), false)], &first);
        assert_eq!(next[0], Shown { kind: Kind::Headset, percent: 53, charging: false, away: true });
        // A device that never had a reading isn't shown.
        assert!(shown(&[reading(Kind::Headset, None, true)], &[]).is_empty());
    }

    #[test]
    fn an_asleep_device_isnt_shown_charging() {
        let r = Reading { kind: Kind::Mouse, battery: Some(40), charging: true, away: true };
        assert!(!shown(&[r], &[])[0].charging);
    }
}
