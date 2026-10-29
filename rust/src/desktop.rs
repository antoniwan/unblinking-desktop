//! The few things we ask the operating system directly: where the cursor is
//! and which part of the screen the taskbar leaves free.
//!
//! All values are physical pixels. winit makes the process per-monitor DPI
//! aware when the event loop starts, so Windows reports real pixels here.

/// A point on the desktop, in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// A rectangle on the desktop, in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

// `#[cfg(windows)]` compiles an item only when building for Windows.
// The `#[cfg(not(windows))]` twins below keep the crate building elsewhere
// (the Mac, later) with honest "don't know" answers instead.

/// The global cursor position, or `None` if Windows won't say
/// (for example while the secure desktop is up).
#[cfg(windows)]
pub fn cursor() -> Option<Point> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let mut p = POINT::default();
    // `unsafe` marks a call the compiler can't check for us: here a raw
    // Win32 function writing through a pointer. `.ok()?` turns its
    // `Result` into an `Option` and returns `None` early on failure.
    unsafe { GetCursorPos(&mut p) }.ok()?;
    Some(Point {
        x: p.x as f64,
        y: p.y as f64,
    })
}

/// The primary monitor's work area: the screen minus the taskbar.
#[cfg(windows)]
pub fn work_area() -> Option<Rect> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };

    let mut r = RECT::default();
    unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut r as *mut RECT as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .ok()?;
    Some(Rect {
        x: r.left as f64,
        y: r.top as f64,
        width: (r.right - r.left) as f64,
        height: (r.bottom - r.top) as f64,
    })
}

/// How many of the primary screen's pixels make a metre, from the size the
/// monitor reports for itself. Falls back to a typical desktop screen.
#[cfg(windows)]
pub fn pixels_per_metre() -> f64 {
    use windows::Win32::Graphics::Gdi::{GetDC, GetDeviceCaps, HORZRES, HORZSIZE, ReleaseDC};
    let (px, mm) = unsafe {
        let dc = GetDC(None);
        let px = GetDeviceCaps(Some(dc), HORZRES);
        let mm = GetDeviceCaps(Some(dc), HORZSIZE);
        ReleaseDC(None, dc);
        (px, mm)
    };
    if px > 0 && mm > 100 {
        px as f64 / (mm as f64 / 1000.0)
    } else {
        4000.0
    }
}

/// Pixels per metre of one monitor (by its Windows device name, such as
/// "\\.\DISPLAY2"), from the size it reports. Falls back to a 54 cm wide screen.
#[cfg(windows)]
pub fn monitor_px_per_m(name: &str, width_px: f64) -> f64 {
    use windows::Win32::Graphics::Gdi::{CreateDCW, DeleteDC, GetDeviceCaps, HORZRES, HORZSIZE};
    use windows::core::{HSTRING, PCWSTR};
    let device = HSTRING::from(name);
    let (px, mm) = unsafe {
        let dc = CreateDCW(&HSTRING::from("DISPLAY"), &device, PCWSTR::null(), None);
        if dc.is_invalid() {
            (0, 0)
        } else {
            let caps = (
                GetDeviceCaps(Some(dc), HORZRES),
                GetDeviceCaps(Some(dc), HORZSIZE),
            );
            let _ = DeleteDC(dc);
            caps
        }
    };
    if px > 0 && mm > 100 {
        px as f64 / (mm as f64 / 1000.0)
    } else {
        width_px / 0.54
    }
}

#[cfg(not(windows))]
pub fn monitor_px_per_m(_name: &str, width_px: f64) -> f64 {
    width_px / 0.54
}

#[cfg(not(windows))]
pub fn pixels_per_metre() -> f64 {
    4000.0
}

/// The hour of the day on the local clock, 0..23.
pub fn local_hour() -> u32 {
    local_time().0
}

/// Hours and minutes on the local clock.
#[cfg(windows)]
pub fn local_time() -> (u32, u32) {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    // Some Win32 calls simply return a struct and can't fail.
    let now = unsafe { GetLocalTime() };
    (now.wHour as u32, now.wMinute as u32)
}

#[cfg(not(windows))]
pub fn local_time() -> (u32, u32) {
    (12, 0)
}

/// Seconds since the last keyboard or mouse input anywhere on the desktop.
#[cfg(windows)]
pub fn idle_secs() -> Option<f64> {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    unsafe { GetLastInputInfo(&mut info) }.ok().ok()?;
    let now = unsafe { GetTickCount() };
    Some(now.wrapping_sub(info.dwTime) as f64 / 1000.0)
}

#[cfg(not(windows))]
pub fn idle_secs() -> Option<f64> {
    None
}

#[cfg(not(windows))]
pub fn cursor() -> Option<Point> {
    None
}

#[cfg(not(windows))]
pub fn work_area() -> Option<Rect> {
    None
}
