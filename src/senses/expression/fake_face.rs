//! A made-up face the expression tests share. Only built for tests (see the
//! `#[cfg(test)]` on `mod fake_face` in `mod.rs`).

use crate::mind::Face;
use crate::senses::Detection;

/// A made-up face for the tests, described in its own frame (units of
/// the distance between the eyes, eyes at y = 0): mouth `width`, mouth
/// corners `mouth_y` below the eyes, face box `length`, nose `yaw`
/// sideways, head tilted by `roll` radians.
#[derive(Clone, Copy)]
pub(super) struct Fake {
    pub width: f32,
    pub mouth_y: f32,
    pub length: f32,
    pub yaw: f32,
    pub roll: f32,
    pub score: f32,
}

pub(super) const REST: Fake = Fake {
    width: 0.83,
    mouth_y: 1.16,
    length: 2.6,
    yaw: 0.0,
    roll: 0.0,
    score: 0.9,
};

pub(super) fn detection(f: Fake) -> Detection {
    let aspect = 16.0 / 9.0;
    // Eyes 0.12 frame heights apart, centred at (0.9, 0.4) in frame
    // heights, which is about a face at a desk.
    let (iod, cx, cy) = (0.12f32, 0.9f32, 0.4f32);
    let (sin, cos) = f.roll.sin_cos();
    let put = |x: f32, y: f32| {
        let (rx, ry) = (x * cos - y * sin, x * sin + y * cos);
        [(cx + rx * iod) / aspect, cy + ry * iod]
    };
    let landmarks = [
        put(-0.5, 0.0),
        put(0.5, 0.0),
        put(f.yaw, 0.57),
        put(-f.width / 2.0, f.mouth_y),
        put(f.width / 2.0, f.mouth_y),
    ];
    let bw = 2.0 * iod / aspect;
    Detection {
        face: Face {
            x: ((landmarks[0][0] + landmarks[1][0]) / 2.0) as f64,
            y: cy as f64,
            size: bw as f64,
            score: f.score as f64,
            aspect: aspect as f64,
        },
        bbox: [cx / aspect - bw / 2.0, cy - 0.9 * iod, bw, f.length * iod],
        landmarks,
    }
}
