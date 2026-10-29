//! Frame pacing: how often the eye draws, so it costs little when it
//! doesn't need to be busy.
//!
//! Left alone, a winit app that asks for a redraw after every frame draws as
//! fast as vsync lets it: 144 frames a second on a 144 Hz monitor, forever.
//! Instead the main loop asks [`frame_interval`] how long to wait between
//! frames, and sleeps until then with `ControlFlow::WaitUntil`. Everything
//! that moves is timed by the clock (each frame gets the real time since the
//! last one), so fewer frames make the motion coarser, never slower.
//!
//! The decision is a plain function of a few yes/no facts, with no window or
//! GPU in sight, so it can be unit tested.

use std::time::{Duration, Instant};

/// While the eye is on screen and something is going on: 60 frames a second.
/// The flames are slow, soft shapes; above 60 they look no different, and on
/// a 144 Hz monitor this alone cuts the GPU work by more than half.
pub const ACTIVE_FRAME: Duration = Duration::from_micros(16_667);

/// Blinded, or nobody around for a while: 30 frames a second. A dim or idle
/// eye drifts slowly enough that half the frames are not missed.
pub const RESTFUL_FRAME: Duration = Duration::from_micros(33_333);

/// Hidden, with nothing to draw: wake this often anyway, to poll the tray,
/// the Ctrl+Alt+B shortcut and "another copy was launched". A quarter second
/// feels instant for a click and costs next to nothing.
pub const HIDDEN_TICK: Duration = Duration::from_millis(250);

/// No face seen and no noise heard for this long counts as calm.
pub const CALM_AFTER: Duration = Duration::from_secs(60);

/// The facts the pacing depends on.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pacing {
    /// The eye is opening or open.
    pub shown: bool,
    /// The eye is in the middle of opening or closing.
    pub fading: bool,
    /// The user blinded it (camera and mic off).
    pub blind: bool,
    /// Nobody seen or heard for a while, and it's only looking around.
    pub calm: bool,
    /// Blinki's Senses or "Where's my camera?" is open. They're drawn along
    /// with the eye's frames, so they need frames even while the eye hides.
    pub egui_open: bool,
}

/// How long between frames, or `None` for "don't draw at all, just tick".
pub fn frame_interval(p: Pacing) -> Option<Duration> {
    if !p.shown && !p.fading {
        // Hidden: draw only for the egui windows, which want to stay snappy.
        return p.egui_open.then_some(ACTIVE_FRAME);
    }
    // Opening and closing are short and should look smooth, and the egui
    // windows answer clicks and typing; both get the full rate.
    if p.fading || p.egui_open {
        return Some(ACTIVE_FRAME);
    }
    if p.blind || p.calm {
        Some(RESTFUL_FRAME)
    } else {
        Some(ACTIVE_FRAME)
    }
}

/// When the frame after one drawn `now` is due, given that this one was due
/// at `due`. Stepping from `due` rather than from `now` keeps a steady beat
/// (a frame that wakes a little late doesn't push every later one back);
/// after a long stall it starts over from `now` instead of rushing to catch up.
pub fn next_due(due: Instant, now: Instant, interval: Duration) -> Instant {
    let next = due + interval;
    if next > now { next } else { now + interval }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown() -> Pacing {
        Pacing {
            shown: true,
            ..Pacing::default()
        }
    }

    #[test]
    fn a_hidden_eye_does_not_draw() {
        assert_eq!(frame_interval(Pacing::default()), None);
        // Blind or calm makes no difference while it's hidden.
        let hidden_blind = Pacing {
            blind: true,
            calm: true,
            ..Pacing::default()
        };
        assert_eq!(frame_interval(hidden_blind), None);
    }

    #[test]
    fn a_hidden_eye_still_draws_for_open_panels() {
        let p = Pacing {
            egui_open: true,
            ..Pacing::default()
        };
        assert_eq!(frame_interval(p), Some(ACTIVE_FRAME));
    }

    #[test]
    fn a_shown_eye_draws_at_sixty() {
        assert_eq!(frame_interval(shown()), Some(ACTIVE_FRAME));
    }

    #[test]
    fn blind_or_calm_draws_at_thirty() {
        let blind = Pacing {
            blind: true,
            ..shown()
        };
        let calm = Pacing {
            calm: true,
            ..shown()
        };
        assert_eq!(frame_interval(blind), Some(RESTFUL_FRAME));
        assert_eq!(frame_interval(calm), Some(RESTFUL_FRAME));
    }

    #[test]
    fn fading_and_panels_keep_the_full_rate() {
        // Closing: no longer "shown", but still on screen and moving.
        let closing = Pacing {
            fading: true,
            calm: true,
            ..Pacing::default()
        };
        assert_eq!(frame_interval(closing), Some(ACTIVE_FRAME));
        let panel = Pacing {
            blind: true,
            egui_open: true,
            ..shown()
        };
        assert_eq!(frame_interval(panel), Some(ACTIVE_FRAME));
    }

    #[test]
    fn the_beat_stays_steady() {
        let t0 = Instant::now();
        let step = ACTIVE_FRAME;
        // Woke 2 ms late: the next frame is still one step after the due time.
        assert_eq!(next_due(t0, t0 + Duration::from_millis(2), step), t0 + step);
        // Stalled for a second: start over from now, no burst of catch-up frames.
        let late = t0 + Duration::from_secs(1);
        assert_eq!(next_due(t0, late, step), late + step);
    }
}
