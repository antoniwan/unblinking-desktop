//! Sight (and, later, hearing): the Rust side of renderer/senses.js.
//! Camera frames go to a face detector running locally. Nothing is
//! recorded, stored, or sent anywhere; each frame is dropped once looked at.

pub mod camera;
pub mod face;
pub mod expression;
pub mod mic;

use image::RgbImage;

use crate::mind::Face;

/// One face as the detector saw it: what the mind needs (`face`) plus the
/// box and landmarks, for drawing in Blinki's Senses. All in 0..1 of the
/// frame, not mirrored.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Detection {
    pub face: Face,
    /// Left, top, width, height.
    pub bbox: [f32; 4],
    /// Right eye, left eye, nose tip, right and left mouth corners.
    pub landmarks: [[f32; 2]; 5],
}

/// Anything that can find faces in a camera frame.
///
/// A *trait* is a set of methods a type promises to have, like an interface.
/// The camera thread only knows "some Detector", so the face model can be
/// swapped (or faked in a test) without touching the camera code.
/// `Send` means a value can be handed to another thread, which the camera
/// thread needs.
pub trait Detector: Send {
    /// Faces as `{ x, y, size, score }`: center in 0..1 of the raw frame (not
    /// mirrored), size as a fraction of frame width, biggest first.
    fn detect(&mut self, frame: &RgbImage) -> Vec<Detection>;
}
