//! The mind: turns what the senses report into how the eye behaves.
//! Ported line for line from the Electron app's mind.js (tag v0.1.0).
//!
//!   Locked   a face is in view; the pupil narrows to a slit and follows it
//!   Hunting  no face; it darts around, follows the cursor, grows restless
//!   Blind    camera and mic are off; embers only, gaze drifting
//!
//! Loud sounds make it flinch and flare. Too close makes it burn.
//! Two faces make it look from one to the other.
//!
//! It speaks through the `say` callback it is given. `update()` returns the
//! per-frame state the eye (and anything else that follows its mood) reads.
//!
//! The pieces:
//!
//!   types     what goes in (`Senses`, `World`, `Face`) and comes out (`Frame`)
//!   faces     where a face is in the room, and how many people are in view
//!   machine   the `Mind` state machine itself
//!   feeling   which mood fits the moment
//!   moods     how each mood looks; themes: who the blinki is
//!   motion    the body that carries out what the mind decides

pub mod feeling;
pub mod moods;
pub mod motion;
pub mod themes;

mod faces;
mod machine;
mod types;

// `pub use` re-exports: callers write `crate::mind::Mind`, not
// `crate::mind::machine::Mind`. The files are how the code is organised;
// these lines are the module's public face.
pub use faces::{REST_SEAT, crowd_size, face_in_room};
pub use machine::{BANG_LEVEL, Mind};
pub use types::{Face, Frame, Insight, Line, Mode, Say, Senses, Vec2, World};

// The two helpers below are private, but private items are visible to the
// module they're in *and every module inside it*, so machine.rs and
// motion.rs can both `use super::approach`.

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    v.max(lo).min(hi)
}

/// Moves `cur` toward `target` at `rate` per second, frame-rate independent.
fn approach(cur: f64, target: f64, rate: f64, dt: f64) -> f64 {
    cur + (target - cur) * (1.0 - (-rate * dt).exp())
}
