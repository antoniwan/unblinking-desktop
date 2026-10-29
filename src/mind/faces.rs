//! Reading faces: where one is in the room, and how many people are in view.

use std::f64::consts::PI;

use super::{Face, Vec2};

/// The camera's horizontal field of view, in radians. Webcams mostly sit
/// between 65 and 80 degrees across in 16:9.
const CAMERA_HFOV: f64 = 70.0 * PI / 180.0;
/// How wide a real face is, as YuNet's box measures it, in metres.
const FACE_WIDTH_M: f64 = 0.15;
/// Where "sitting normally" puts your eyes: straight out from the camera,
/// a little below it. "Look at me" in the menu moves you here.
pub const REST_SEAT: Vec2 = Vec2::new(0.0, 0.08);

/// A second face counts as company only if it's at least this big next to
/// the biggest one (about the same distance from the camera).
/// `pub(super)`: the mind's state machine uses it too, nothing else does.
pub(super) const CROWD_SIZE: f64 = 0.8;

/// Where a face is in the room, in metres from the camera: x to the right
/// and y down as you see the screen, and z toward you. The camera is
/// assumed to face you square on, wherever it's clipped.
///
/// The camera image isn't mirrored, so a face on the image's left is on
/// your right. Distance comes from how wide the face looks.
pub fn face_in_room(face: &Face) -> [f64; 3] {
    let tan_h = (CAMERA_HFOV / 2.0).tan();
    let tan_v = tan_h / face.aspect.max(0.1);
    let dist = FACE_WIDTH_M / (face.size.max(0.02) * 2.0 * tan_h);
    let x = (0.5 - face.x) * 2.0 * tan_h * dist;
    let y = (face.y - 0.5) * 2.0 * tan_v * dist;
    [x, y, dist]
}

/// How many people are in view: confident faces near the biggest one's size.
pub fn crowd_size(faces: &[Face]) -> usize {
    let sure = || faces.iter().filter(|f| f.score > 0.55);
    let biggest = sure().map(|f| f.size).fold(0.0, f64::max);
    sure().filter(|f| f.size >= biggest * CROWD_SIZE).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(size: f64, score: f64) -> Face {
        Face {
            x: 0.5,
            y: 0.5,
            size,
            score,
            aspect: 16.0 / 9.0,
        }
    }

    #[test]
    fn a_closer_face_is_nearer_the_camera() {
        let near = face_in_room(&face(0.4, 0.9));
        let far = face_in_room(&face(0.1, 0.9));
        assert!(near[2] < far[2]);
        assert!((far[2] / near[2] - 4.0).abs() < 1e-9);
    }

    #[test]
    fn crowd_size_counts_confident_faces_of_a_similar_size() {
        assert_eq!(crowd_size(&[]), 0);
        assert_eq!(crowd_size(&[face(0.2, 0.9)]), 1);
        assert_eq!(crowd_size(&[face(0.2, 0.9), face(0.17, 0.8)]), 2);
        // Too small next to the biggest (a photo on the wall), or unsure.
        assert_eq!(crowd_size(&[face(0.2, 0.9), face(0.1, 0.9)]), 1);
        assert_eq!(crowd_size(&[face(0.2, 0.9), face(0.2, 0.4)]), 1);
    }
}
