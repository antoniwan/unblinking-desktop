//! How the app lives on the computer: only one copy at a time, and,
//! if you ask for it, starting when you log in.

/// Held for the life of the first copy. A second launch finds it taken,
/// pokes the first copy awake (as Electron's 'second-instance' did), and leaves.
pub struct SingleInstance {
    #[cfg(windows)]
    _mutex: windows::Win32::Foundation::HANDLE,
    #[cfg(windows)]
    wake: windows::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
const MUTEX: windows::core::PCWSTR = windows::core::w!("Local\\Unblinking.SingleInstance");
#[cfg(windows)]
const WAKE: windows::core::PCWSTR = windows::core::w!("Local\\Unblinking.Wake");

impl SingleInstance {
    /// `Some` if this is the first copy. `None` means another copy is
    /// running; it has been told to show itself, and this one should exit.
    #[cfg(windows)]
    pub fn claim() -> Option<SingleInstance> {
        use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
        use windows::Win32::System::Threading::{CreateEventW, CreateMutexW, SetEvent};
        unsafe {
            let mutex = CreateMutexW(None, true, MUTEX).ok()?;
            // An auto-reset event: each SetEvent wakes the first copy once.
            let wake = CreateEventW(None, false, false, WAKE).ok()?;
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = SetEvent(wake);
                return None;
            }
            Some(SingleInstance {
                _mutex: mutex,
                wake,
            })
        }
    }

    #[cfg(not(windows))]
    pub fn claim() -> Option<SingleInstance> {
        Some(SingleInstance {})
    }

    /// True once each time another copy was launched since the last call.
    #[cfg(windows)]
    pub fn woken(&self) -> bool {
        use windows::Win32::Foundation::WAIT_OBJECT_0;
        use windows::Win32::System::Threading::WaitForSingleObject;
        // A zero timeout: just look, never wait.
        unsafe { WaitForSingleObject(self.wake, 0) == WAIT_OBJECT_0 }
    }

    #[cfg(not(windows))]
    pub fn woken(&self) -> bool {
        false
    }
}

// ---- Start with the computer: the per-user Run key, as Electron's
// setLoginItemSettings did. Mac: not written yet (untested there anyway).

#[cfg(windows)]
const RUN_KEY: windows::core::PCWSTR =
    windows::core::w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
#[cfg(windows)]
const RUN_NAME: windows::core::PCWSTR = windows::core::w!("Unblinking");

/// Whether Windows starts the eye when you log in.
#[cfg(windows)]
pub fn starts_with_computer() -> bool {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            RUN_NAME,
            RRF_RT_REG_SZ,
            None,
            None,
            None,
        )
    }
    .is_ok()
}

/// Adds or removes this exe from the per-user Run key.
#[cfg(windows)]
pub fn set_starts_with_computer(on: bool) -> Result<(), String> {
    use windows::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RegDeleteKeyValueW, RegSetKeyValueW,
    };
    if !on {
        let r = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_NAME) };
        return if r.is_ok() || r.0 == 2 {
            Ok(())
        } else {
            Err(format!("registry error {}", r.0))
        }; // 2: wasn't there
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    // Quoted, as a path with spaces must be; UTF-16 with a closing zero.
    let value: Vec<u16> = format!("\"{}\"", exe.display())
        .encode_utf16()
        .chain([0])
        .collect();
    let bytes = unsafe { std::slice::from_raw_parts(value.as_ptr() as *const u8, value.len() * 2) };
    let r = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            RUN_NAME,
            REG_SZ.0,
            Some(bytes.as_ptr() as *const _),
            bytes.len() as u32,
        )
    };
    if r.is_ok() {
        Ok(())
    } else {
        Err(format!("registry error {}", r.0))
    }
}

#[cfg(not(windows))]
pub fn starts_with_computer() -> bool {
    false
}

#[cfg(not(windows))]
pub fn set_starts_with_computer(_on: bool) -> Result<(), String> {
    Err("not written for this system yet".into())
}
