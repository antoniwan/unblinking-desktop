//! Sight (and, later, hearing): the Rust side of renderer/senses.js.
//! Camera frames go to a face detector running locally. Nothing is
//! recorded, stored, or sent anywhere; each frame is dropped once looked at.

pub mod camera;
pub mod face;
pub mod mic;

use image::RgbImage;

use crate::mind::Face;

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
    fn detect(&mut self, frame: &RgbImage) -> Vec<Face>;
}
