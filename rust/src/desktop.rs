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
    Some(Point { x: p.x as f64, y: p.y as f64 })
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

#[cfg(not(windows))]
pub fn cursor() -> Option<Point> {
    None
}

#[cfg(not(windows))]
pub fn work_area() -> Option<Rect> {
    None
}
