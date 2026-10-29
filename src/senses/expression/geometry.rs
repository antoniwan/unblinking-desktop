//! The first reader: the face's shape from YuNet's five landmarks, and the
//! resting shape it is compared against.

use crate::senses::Detection;

/// How slowly the resting shape adapts, in seconds, once it is settled.
pub(super) const NEUTRAL_TAU: f32 = 20.0;
/// How fast it adapts when the face is clearly at rest (or the model says so).
pub(super) const NEUTRAL_TAU_FAST: f32 = 4.0;
/// Frames of resting face before the reading is fully trusted (~1.3 s).
const WARM_SAMPLES: f32 = 20.0;

/// The shape of a face, measured from YuNet's landmarks in units of the
/// distance between the eyes, so it doesn't change with how near you sit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Shape {
    /// Mouth corner to mouth corner (about 0.83 on a resting face).
    width: f32,
    /// Where the mouth corners sit below the nose, as a share of the whole
    /// eyes-to-mouth drop (about 0.5). Smiling lifts the corners, so it
    /// shrinks. A share rather than a distance, so nodding doesn't move it.
    drop: f32,
    /// The face box's height (about 2.5 to 3). A dropped jaw makes it longer.
    length: f32,
    /// Nose tip left or right of the middle of the eyes (0 = straight on).
    yaw: f32,
    /// Tilt of the line between the eyes, in radians.
    roll: f32,
}

/// Measures a detection. `None` when the landmarks make no sense (eyes on
/// top of each other, mouth above the eyes), which happens on bad frames.
pub(super) fn shape(d: &Detection) -> Option<Shape> {
    // Landmarks are fractions of the frame's width and height. Stretch x by
    // the frame's aspect so both axes use the same unit (frame heights).
    let aspect = if d.face.aspect > 0.0 {
        d.face.aspect as f32
    } else {
        1.0
    };
    let p = d.landmarks.map(|[x, y]| [x * aspect, y]);
    let mid = [(p[0][0] + p[1][0]) / 2.0, (p[0][1] + p[1][1]) / 2.0];
    let mut along = [p[1][0] - p[0][0], p[1][1] - p[0][1]];
    let iod = along[0].hypot(along[1]);
    if iod < 1e-4 {
        return None;
    }
    // Point the eye line left to right in the picture whichever eye is which.
    if along[0] < 0.0 {
        along = [-along[0], -along[1]];
    }
    let ux = [along[0] / iod, along[1] / iod];
    // At right angles to the eye line, pointing down the face.
    let uy = [-ux[1], ux[0]];
    // A point in the face's own frame: the eyes at (-0.5, 0) and (0.5, 0),
    // the face upright even when the head tilts.
    let local = |q: [f32; 2]| {
        let (dx, dy) = (q[0] - mid[0], q[1] - mid[1]);
        [
            (dx * ux[0] + dy * ux[1]) / iod,
            (dx * uy[0] + dy * uy[1]) / iod,
        ]
    };
    let (nose, m0, m1) = (local(p[2]), local(p[3]), local(p[4]));
    let mouth_y = (m0[1] + m1[1]) / 2.0;
    if mouth_y < 0.2 {
        return None;
    }
    Some(Shape {
        width: (m1[0] - m0[0]).hypot(m1[1] - m0[1]),
        drop: (mouth_y - nose[1]) / mouth_y,
        length: d.bbox[3] / iod,
        yaw: nose[0],
        roll: ux[1].atan2(ux[0]),
    })
}

/// 0 below `lo`, 1 above `hi`, and a smooth S-curve between.
pub(super) fn smoothstep(lo: f32, hi: f32, x: f32) -> f32 {
    let t = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How much a face is turned toward the camera, from its shape alone.
pub(super) fn facing(s: &Shape) -> f32 {
    let yaw = 1.0 - smoothstep(0.06, 0.45, s.yaw.abs());
    let roll = 1.0 - smoothstep(0.3, 0.75, s.roll.abs());
    yaw * roll
}

/// This face at rest, learned as the session goes. A running average that
/// mostly listens to frames that look calm.
#[derive(Clone, Copy, Debug)]
pub(super) struct Neutral {
    width: f32,
    drop: f32,
    length: f32,
    /// How many frames have gone into it, up to `WARM_SAMPLES`.
    samples: f32,
}

impl Neutral {
    /// A resting shape that starts as `s` and has learned nothing yet.
    pub(super) fn new(s: &Shape) -> Neutral {
        Neutral {
            width: s.width,
            drop: s.drop,
            length: s.length,
            samples: 0.0,
        }
    }

    /// Signed smile and surprise for `s` compared with rest: 0 at rest,
    /// about 1 for a clear expression, negative for "less than at rest".
    /// Also how much longer the face is than at rest (0.1 = 10% longer).
    pub(super) fn compare(&self, s: &Shape) -> (f32, f32, f32) {
        let wider = (s.width - self.width) / self.width;
        let lifted = self.drop - s.drop;
        let longer = (s.length - self.length) / self.length;
        let smile = 0.6 * wider / 0.15 + 0.4 * lifted / 0.08;
        let surprise = 0.55 * longer / 0.10 - 0.25 * wider / 0.12 - 0.2 * lifted / 0.08;
        (smile, surprise, longer)
    }

    /// Moves the resting shape a little toward `s`, by how much `rate` says.
    pub(super) fn learn(&mut self, s: &Shape, rate: f32) {
        // The first frames are averaged evenly, so it settles in a second.
        let rate = if self.samples < WARM_SAMPLES {
            1.0 / (self.samples + 1.0)
        } else {
            rate.min(1.0)
        };
        self.width += (s.width - self.width) * rate;
        self.drop += (s.drop - self.drop) * rate;
        self.length += (s.length - self.length) * rate;
        self.samples = (self.samples + 1.0).min(WARM_SAMPLES);
    }

    /// How settled the resting shape is: 0 with nothing learned, 1 once
    /// `WARM_SAMPLES` frames have gone in.
    pub(super) fn warmth(&self) -> f32 {
        (self.samples / WARM_SAMPLES).min(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::super::fake_face::{Fake, REST, detection};
    use super::*;

    #[test]
    fn a_resting_face_measures_like_the_template() {
        let s = shape(&detection(REST)).unwrap();
        assert!((s.width - 0.83).abs() < 1e-3, "{s:?}");
        assert!((s.drop - (1.0 - 0.57 / 1.16)).abs() < 1e-3, "{s:?}");
        assert!((s.length - 2.6).abs() < 1e-3 && s.yaw.abs() < 1e-3, "{s:?}");
        // A tilted head measures the same, apart from the tilt.
        let tilted = shape(&detection(Fake { roll: 0.3, ..REST })).unwrap();
        assert!((tilted.width - s.width).abs() < 1e-3);
        assert!((tilted.roll - 0.3).abs() < 1e-3);
    }
}
