//! Expression: a rough, on-device reading of the face the eye is watching.
//! Only while the user has opted in. Nothing is stored or sent.

/// What the main face seems to be doing. All 0..1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Expression {
    /// Not smiling (0) to beaming (1).
    pub smile: f32,
    /// Calm (0) to startled, mouth open (1).
    pub surprise: f32,
    /// Turned away (0) to looking straight at the screen (1).
    pub facing: f32,
    /// How much to trust this reading (0 = ignore it).
    pub confidence: f32,
}
