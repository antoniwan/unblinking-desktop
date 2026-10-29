//! The window's own life: opening, open, closing, hidden.

use std::time::Instant;

use winit::event_loop::ActiveEventLoop;

use super::*;
use crate::bond::remembrance::{self};
use crate::render::gpu::Gpu;

/// How long the eye takes to open when it appears.
const OPEN_SECS: f32 = 0.9;
/// How long it takes to close before it hides or quits.
const CLOSE_SECS: f32 = 0.6;

/// What to do once the eye has finished closing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum AfterClose {
    Hide,
    Quit,
}

/// The window's own life, apart from the mind's moods. An enum variant can
/// carry data: `Opening` remembers when it started, `Closing` also what
/// comes after.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Presence {
    Opening(Instant),
    Open,
    Closing(Instant, AfterClose),
    Hidden,
}

impl Presence {
    /// How present the eye is, 0 (gone) to 1 (fully there), eased so it
    /// starts quickly and settles softly.
    pub(crate) fn amount(self) -> f32 {
        match self {
            Presence::Opening(since) => {
                let x = (since.elapsed().as_secs_f32() / OPEN_SECS).min(1.0);
                1.0 - (1.0 - x).powi(3) // ease out
            }
            Presence::Open => 1.0,
            Presence::Closing(since, _) => {
                let x = (since.elapsed().as_secs_f32() / CLOSE_SECS).min(1.0);
                1.0 - x * x // ease in: slow to start, quick at the end
            }
            Presence::Hidden => 0.0,
        }
    }

    pub(crate) fn shown(self) -> bool {
        matches!(self, Presence::Opening(_) | Presence::Open)
    }
}

impl App {
    /// Shows the window (after a clear frame, so no black box) and opens the eye.
    pub(super) fn appear(&mut self) {
        if let Some(gpu) = &mut self.gpu
            && self.presence == Presence::Hidden
        {
            gpu.clear();
            gpu.window.set_visible(true);
            gpu.window.request_redraw();
        }
        self.presence = Presence::Opening(Instant::now());
    }

    /// Starts closing the eye; the frame loop hides or quits once it's done.
    pub(super) fn close(&mut self, after: AfterClose, event_loop: &ActiveEventLoop) {
        if after == AfterClose::Quit
            && let Some(book) = &mut self.book
        {
            book.save(remembrance::unix_now());
        }
        if self.presence == Presence::Hidden {
            // Nothing on screen to animate.
            if after == AfterClose::Quit {
                event_loop.exit();
            }
            return;
        }
        self.presence = Presence::Closing(Instant::now(), after);
    }

    /// After a frame: finish an opening, or end a closing with a clear frame.
    pub(super) fn advance_presence(&mut self, gpu: &mut Gpu, event_loop: &ActiveEventLoop) {
        match self.presence {
            Presence::Opening(since) if since.elapsed().as_secs_f32() >= OPEN_SECS => {
                self.presence = Presence::Open;
            }
            Presence::Closing(since, after) if since.elapsed().as_secs_f32() >= CLOSE_SECS => {
                gpu.clear();
                gpu.window.set_visible(false);
                self.presence = Presence::Hidden;
                if after == AfterClose::Quit {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }
}
