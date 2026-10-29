//! Full-screen jealousy. When you start a full-screen game, a full-screen
//! video or a presentation, the blinki gets out of the way: it closes and
//! hides. When you come back, it fades in again, pouting for a moment.
//!
//! Two layers:
//! - `Jealousy`, a small state machine that only sees "busy or not" and the
//!   time. It knows nothing about Windows, so the tests below can drive it.
//! - `Watcher`, which asks Windows about once a second and feeds `Jealousy`.

use std::time::{Duration, Instant};

/// Busy this long before the eye hides, so a quick alt-tab doesn't count.
const HIDE_AFTER: Duration = Duration::from_secs(2);
/// Clear this long before it comes back.
const RETURN_AFTER: Duration = Duration::from_secs(3);
/// How often to ask Windows. It's cheap, but there's no need every frame.
const POLL_EVERY: Duration = Duration::from_secs(1);
/// How long it sulks after coming back (the opening takes 0.9 s of this).
const POUT_FOR: Duration = Duration::from_secs(8);

/// What it says when it comes back, and the studio mood it sulks in.
const POUTS: &[(&str, &str)] = &[
    ("You left me for a game.", "Lonely"),
    ("I saw nothing. I was not jealous.", "Suspicious"),
    ("Back from your other world?", "Suspicious"),
    ("I waited. In the dark. It was fine.", "Lonely"),
    ("Was it worth it?", "Suspicious"),
];

/// What the app should do with the eye.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Do {
    /// Close and hide it (without touching any saved pref).
    Hide,
    /// Bring it back, pouting.
    Return,
}

/// The decision part, with no Windows in it: feed it what the screen is
/// doing, whether the feature is on and whether the eye is showing, and it
/// says when to hide or bring the eye back.
#[derive(Debug, Default)]
pub struct Jealousy {
    /// Since when the screen has looked busy (or clear), without a break.
    busy_since: Option<Instant>,
    clear_since: Option<Instant>,
    /// The debounced answer: is something full screen right now?
    busy: bool,
    /// We hid the eye, so it's ours to bring back.
    hid_it: bool,
    /// Already decided what to do about this full-screen spell, so whatever
    /// the user does with the eye until it ends is left alone.
    decided: bool,
}

impl Jealousy {
    /// One reading. `raw_busy` is the undebounced answer from Windows.
    pub fn update(
        &mut self,
        raw_busy: bool,
        now: Instant,
        enabled: bool,
        shown: bool,
    ) -> Option<Do> {
        // Debounce: the answer only flips after it has held long enough.
        // `get_or_insert` keeps the first time of an unbroken run.
        if raw_busy {
            self.clear_since = None;
            let since = *self.busy_since.get_or_insert(now);
            if !self.busy && now - since >= HIDE_AFTER {
                self.busy = true;
            }
        } else {
            self.busy_since = None;
            let since = *self.clear_since.get_or_insert(now);
            if self.busy && now - since >= RETURN_AFTER {
                self.busy = false;
            }
        }

        // Switched off while we had it hidden: give it back right away.
        if !enabled || !self.busy {
            self.decided = false;
            return self.give_back(shown);
        }
        if !self.decided {
            // The start of a full-screen spell: decide once. Showing means
            // hide it; already hidden means the user hid it, so it's theirs.
            self.decided = true;
            if shown {
                self.hid_it = true;
                return Some(Do::Hide);
            }
        } else if self.hid_it && shown {
            // We hid it, yet it's showing: the user brought it back from
            // the tray. Their call; it stays until this spell ends.
            self.hid_it = false;
        }
        None
    }

    /// Returns the eye if we were the ones who hid it. If the user hid it
    /// themselves, it was never ours, so `hid_it` is false and it stays put.
    fn give_back(&mut self, shown: bool) -> Option<Do> {
        if !std::mem::take(&mut self.hid_it) {
            return None;
        }
        (!shown).then_some(Do::Return)
    }
}

/// Watches Windows for full-screen apps and keeps the pout going after.
pub struct Watcher {
    jealousy: Jealousy,
    last_poll: Option<Instant>,
    /// Until when it sulks, and the line it hasn't said yet.
    pout: Option<(Instant, &'static str, Option<&'static str>)>,
}

impl Watcher {
    pub fn new() -> Watcher {
        Watcher {
            jealousy: Jealousy::default(),
            last_poll: None,
            pout: None,
        }
    }

    /// Call every frame; it asks Windows at most once a second.
    pub fn tick(&mut self, enabled: bool, shown: bool) -> Option<Do> {
        let now = Instant::now();
        if self.last_poll.is_some_and(|t| now - t < POLL_EVERY) {
            return None;
        }
        self.last_poll = Some(now);
        // Only ask Windows while the feature is on.
        let busy = enabled && something_full_screen();
        let what = self.jealousy.update(busy, now, enabled, shown);
        if what == Some(Do::Return) {
            let (line, mood) = POUTS[fastrand::usize(..POUTS.len())];
            self.pout = Some((now + POUT_FOR, mood, Some(line)));
        }
        what
    }

    /// The mood to sulk in, while it's sulking.
    pub fn sulk(&self) -> Option<&'static str> {
        self.pout
            .filter(|(until, _, _)| Instant::now() < *until)
            .map(|(_, mood, _)| mood)
    }

    /// The jealous line, once (the app says it when the eye is open).
    pub fn take_line(&mut self) -> Option<&'static str> {
        // A line not said while sulking (hidden again before it opened) is dropped.
        let (until, _, line) = self.pout.as_mut()?;
        let line = line.take()?;
        (Instant::now() < *until).then_some(line)
    }
}

/// Is a full-screen game, video or presentation in front right now?
#[cfg(windows)]
fn something_full_screen() -> bool {
    use windows::Win32::UI::Shell::{
        QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
        SHQueryUserNotificationState,
    };
    // The shell's own answer to "may I bother the user?". `matches!` with
    // `Ok(A | B | C)` checks for success *and* one of those three states.
    let state = unsafe { SHQueryUserNotificationState() };
    matches!(
        state,
        Ok(QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE)
    ) || borderless_full_screen()
}

/// Many games and video players are "borderless full screen": a plain window
/// exactly the size of its monitor, which the shell doesn't always report.
#[cfg(windows)]
fn borderless_full_screen() -> bool {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONULL, MONITORINFO, MonitorFromWindow,
    };
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId,
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return false;
        }
        // Not one of our own windows (the eye, Blinki's Senses, the picker).
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == GetCurrentProcessId() {
            return false;
        }
        // Not the desktop itself, which is also "a window the size of the screen".
        let mut class = [0u16; 64];
        let len = GetClassNameW(hwnd, &mut class).max(0) as usize;
        let class = String::from_utf16_lossy(&class[..len]);
        if class == "Progman" || class == "WorkerW" {
            return false;
        }
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return false;
        }
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONULL);
        if monitor.is_invalid() {
            return false;
        }
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return false;
        }
        // The whole monitor, taskbar included. A maximised window stops at
        // the taskbar (and pokes a few pixels past the edges), so it won't match.
        rect == info.rcMonitor
    }
}

#[cfg(not(windows))]
fn something_full_screen() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds one reading per second from `start`, `secs` times, and collects
    /// whatever the state machine says.
    fn run(
        j: &mut Jealousy,
        start: Instant,
        from: u64,
        secs: u64,
        busy: bool,
        shown: &mut bool,
    ) -> Vec<(u64, Do)> {
        let mut out = Vec::new();
        for s in from..from + secs {
            if let Some(d) = j.update(busy, start + Duration::from_secs(s), true, *shown) {
                // Act as the app would.
                *shown = d == Do::Return;
                out.push((s, d));
            }
        }
        out
    }

    #[test]
    fn hides_after_two_seconds_and_returns_after_three() {
        let t = Instant::now();
        let mut j = Jealousy::default();
        let mut shown = true;
        assert_eq!(run(&mut j, t, 0, 10, true, &mut shown), vec![(2, Do::Hide)]);
        assert!(!shown);
        assert_eq!(
            run(&mut j, t, 10, 10, false, &mut shown),
            vec![(13, Do::Return)]
        );
        assert!(shown);
    }

    #[test]
    fn a_quick_alt_tab_does_nothing() {
        let t = Instant::now();
        let mut j = Jealousy::default();
        let mut shown = true;
        // Busy for a second, clear, busy for a second: never two in a row.
        for s in 0..20 {
            let busy = s % 2 == 0;
            let out = j.update(busy, t + Duration::from_secs(s), true, shown);
            assert_eq!(out, None, "at {s}s");
        }
        // And while hidden, a short look at the desktop doesn't bring it back.
        run(&mut j, t, 20, 5, true, &mut shown);
        assert!(!shown);
        assert!(run(&mut j, t, 25, 2, false, &mut shown).is_empty());
        assert!(run(&mut j, t, 27, 3, true, &mut shown).is_empty());
        assert!(!shown);
    }

    #[test]
    fn leaves_an_eye_the_user_hid_alone() {
        let t = Instant::now();
        let mut j = Jealousy::default();
        let mut shown = false; // hidden from the tray before the game
        assert!(run(&mut j, t, 0, 10, true, &mut shown).is_empty());
        assert!(run(&mut j, t, 10, 10, false, &mut shown).is_empty());
        assert!(!shown, "still hidden, as the user left it");
    }

    #[test]
    fn showing_a_user_hidden_eye_mid_game_is_respected() {
        let t = Instant::now();
        let mut j = Jealousy::default();
        let mut shown = false;
        assert!(run(&mut j, t, 0, 5, true, &mut shown).is_empty());
        shown = true; // the user shows it while the game runs
        assert!(run(&mut j, t, 5, 10, true, &mut shown).is_empty());
        assert!(shown);
    }

    #[test]
    fn showing_it_during_a_game_is_respected() {
        let t = Instant::now();
        let mut j = Jealousy::default();
        let mut shown = true;
        run(&mut j, t, 0, 5, true, &mut shown);
        assert!(!shown);
        shown = true; // the user clicks "Show the eye"
        assert!(run(&mut j, t, 5, 20, true, &mut shown).is_empty());
        assert!(shown, "not hidden again during the same game");
        // The game ends, then another starts: back to hiding.
        assert!(run(&mut j, t, 25, 5, false, &mut shown).is_empty());
        assert_eq!(
            run(&mut j, t, 30, 5, true, &mut shown),
            vec![(32, Do::Hide)]
        );
    }

    #[test]
    fn switching_it_off_gives_the_eye_back() {
        let t = Instant::now();
        let mut j = Jealousy::default();
        let mut shown = true;
        run(&mut j, t, 0, 5, true, &mut shown);
        assert_eq!(
            j.update(true, t + Duration::from_secs(5), false, false),
            Some(Do::Return)
        );
        assert_eq!(
            j.update(true, t + Duration::from_secs(6), false, true),
            None
        );
    }

    #[test]
    fn every_pout_mood_exists() {
        let moods = crate::mind::moods::Moods::load();
        let names: Vec<&str> = moods
            .theme()
            .moods
            .iter()
            .map(|m| m.name.as_str())
            .collect();
        for (line, mood) in POUTS {
            assert!(names.contains(mood), "no mood {mood}");
            assert!(!line.contains('\u{2014}'), "no em dashes in {line}");
        }
    }
}
