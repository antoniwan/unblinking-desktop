//! What goes into the mind each frame and what comes out of it: the plain
//! data types the rest of the app shares with `Mind`.

use crate::platform::desktop::{Point, Rect};

/// What the eye is doing. A Rust `enum` is a closed set of choices; unlike
/// the strings the Electron app used, a typo here is a compile error, and a
/// `match` that forgets a mode won't compile either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Locked,
    Hunting,
    Blind,
}

/// The kinds of things it whispers.
/// `Hash` and `Eq` let a `Line` be a key in a HashMap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Line {
    First,
    Returned,
    Gone,
    Close,
    Crowd,
    Loud,
    Talking,
    Long,
    NoSight,
    Unblind,
    /// Someone said the wake word.
    Called,
}

impl Line {
    /// The words for each kind. `&'static str` is text baked into the
    /// program itself, so it lives (is valid) for the whole run.
    pub fn texts(self) -> &'static [&'static str] {
        match self {
            Line::First => &["I see you."],
            Line::Returned => &[
                "There you are.",
                "You came back.",
                "I knew you would return.",
            ],
            Line::Gone => &["Where are you?", "You cannot hide.", "I will find you."],
            Line::Close => &["Too close.", "Closer, then."],
            Line::Crowd => &["Who is with you?", "I see two of you."],
            Line::Loud => &["Silence.", "I heard that."],
            Line::Talking => &["I hear you.", "Speak up."],
            Line::Long => &["Still watching.", "I have not looked away."],
            Line::NoSight => &["I cannot see you. Yet."],
            Line::Unblind => &["I see again."],
            Line::Called => &["You called?", "I am here.", "Speak, and I listen."],
        }
    }
}

/// A 2D value: a gaze direction (-1..1, +y up) or a face position.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    pub const fn new(x: f64, y: f64) -> Self {
        Vec2 { x, y }
    }
}

/// One face the camera found: where its eyes are in the frame (0..1, as the
/// camera sees it, not mirrored), its width as a fraction of the frame
/// width, the detector's score, and the frame's width over its height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Face {
    pub x: f64,
    pub y: f64,
    pub size: f64,
    pub score: f64,
    pub aspect: f64,
}

/// Where things are on the desktop, in physical pixels.
#[derive(Clone, Copy, Debug)]
pub struct World {
    pub cursor: Point,
    /// The eye's spot on screen.
    pub eye: Point,
    /// Bounds of the monitor the eye is on.
    pub display: Rect,
    /// Physical pixels per logical pixel. The Electron app measured in
    /// logical pixels; the mind's pixel constants are multiplied by this so
    /// they mean the same distance on a high-DPI screen.
    pub scale: f64,
    /// Where the webcam is on the desktop (see placement.rs).
    pub camera: Point,
    /// Screen pixels per metre, to place the eye in the room.
    pub px_per_m: f64,
    /// Subtracted from where faces seem to be, in metres: the "Look at me"
    /// calibration for a camera that isn't quite where we assume.
    pub look_offset: Vec2,
}

/// Everything the mind is told each frame. The `'a` is a *lifetime*:
/// `faces` borrows a slice someone else owns, and `Senses` can't outlive
/// that slice.
pub struct Senses<'a> {
    pub faces: &'a [Face],
    pub can_see: bool,
    /// Loudness, 0..1.
    pub level: f64,
    pub world: Option<World>,
    pub blind: bool,
    pub whispers: bool,
}

/// What `Mind::update()` hands back each frame.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub gaze: Vec2,
    pub pupil: f64,
    pub heat: f64,
    pub flare: f64,
    pub wide: f64,
    pub dim: f64,
    pub shake: Vec2,
    pub mode: Mode,
}

/// The function the mind talks through. `Box<dyn FnMut(..)>` holds any
/// closure with that signature; `FnMut` means it may change its own
/// captured state each call (for example, push onto a list).
pub type Say = Box<dyn FnMut(&'static str, Line)>;

/// A read-only look inside the mind, for Blinki's Senses.
#[derive(Clone, Debug)]
pub struct Insight {
    /// Seconds since the mind woke.
    pub t: f64,
    /// Where the gaze is heading, before smoothing and tremor.
    pub target: Vec2,
    /// The loudness it has got used to; a bang is a jump above this.
    pub baseline: f64,
    /// When it last flinched, in mind seconds.
    pub last_startle: f64,
    /// The face it's following, smoothed.
    pub tracked: Option<Face>,
    /// Each line kind's earliest next whisper, in mind seconds.
    pub cooldowns: Vec<(Line, f64)>,
    /// No whisper at all before this (the six second hush).
    pub hush_until: f64,
}
