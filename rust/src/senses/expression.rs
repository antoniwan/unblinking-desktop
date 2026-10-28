//! Expression: a rough, on-device reading of the face the eye is watching.
//! Only while the user has opted in. Nothing is stored or sent.
//!
//! Two readers work together, in layers:
//!
//! 1. **Geometry** always runs. YuNet already gives five landmarks (eyes,
//!    nose tip, mouth corners). A smile pulls the mouth corners apart and up;
//!    a dropped jaw makes the face box longer and the mouth narrower. Faces
//!    differ a lot, so instead of fixed thresholds we compare against *this*
//!    face's own resting shape, learned while the session runs.
//! 2. **A small model** (OpenCV Zoo's facial expression recogniser, a
//!    MobileFaceNet, Apache 2.0) looks at the face itself about 4 times a
//!    second and says how happy, surprised or neutral it looks. Its answer
//!    is blended over the geometry and also tells the geometry when the
//!    face is at rest, so it learns the resting shape faster.
//!
//! Everything lives in a [`Feeling`], which the camera thread keeps only
//! while the user has said yes. Turning it off drops the Feeling, and with it
//! everything learned about the face. Frames are only borrowed, never kept.

use std::sync::Arc;
use std::time::{Duration, Instant};

use image::RgbImage;
use tract_onnx::prelude::*;

use super::Detection;

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

/// What a model makes of one face: chances (0..1) for the three
/// expressions the eye cares about. They need not add up to 1, since a
/// model may know other expressions too.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reading {
    pub happy: f32,
    pub surprised: f32,
    pub neutral: f32,
}

/// Anything that can read an expression from a face in a frame.
///
/// Like [`super::Detector`], this is a trait so the model can be swapped,
/// left out, or faked in a test. `Option<Reading>` lets a reader say
/// "couldn't tell this time" without that being an error.
pub trait ExpressionReader: Send {
    fn read(&mut self, frame: &RgbImage, face: &Detection) -> Option<Reading>;
}

/// The camera runs about 15 times a second; the model only needs 4.
const MODEL_EVERY: Duration = Duration::from_millis(250);
/// A model reading older than this is stale and no longer blended in.
const MODEL_FRESH: Duration = Duration::from_millis(750);
/// How much the model's answer counts against the geometry's, when fresh.
const MODEL_SMILE_WEIGHT: f32 = 0.6;
const MODEL_SURPRISE_WEIGHT: f32 = 0.5;

/// How quickly the reading follows the face, in seconds. Lower is jumpier.
const SMOOTH_TAU: f32 = 0.25;
/// How slowly the resting shape adapts, in seconds, once it is settled.
const NEUTRAL_TAU: f32 = 20.0;
/// How fast it adapts when the face is clearly at rest (or the model says so).
const NEUTRAL_TAU_FAST: f32 = 4.0;
/// Frames of resting face before the reading is fully trusted (~1.3 s).
const WARM_SAMPLES: f32 = 20.0;
/// After this long without a face, the smoothed reading starts over.
const LOST_AFTER: Duration = Duration::from_secs(1);

/// The shape of a face, measured from YuNet's landmarks in units of the
/// distance between the eyes, so it doesn't change with how near you sit.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Shape {
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
fn shape(d: &Detection) -> Option<Shape> {
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
fn smoothstep(lo: f32, hi: f32, x: f32) -> f32 {
    let t = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How much a face is turned toward the camera, from its shape alone.
fn facing(s: &Shape) -> f32 {
    let yaw = 1.0 - smoothstep(0.06, 0.45, s.yaw.abs());
    let roll = 1.0 - smoothstep(0.3, 0.75, s.roll.abs());
    yaw * roll
}

/// This face at rest, learned as the session goes. A running average that
/// mostly listens to frames that look calm.
#[derive(Clone, Copy, Debug)]
struct Neutral {
    width: f32,
    drop: f32,
    length: f32,
    /// How many frames have gone into it, up to `WARM_SAMPLES`.
    samples: f32,
}

impl Neutral {
    /// Signed smile and surprise for `s` compared with rest: 0 at rest,
    /// about 1 for a clear expression, negative for "less than at rest".
    /// Also how much longer the face is than at rest (0.1 = 10% longer).
    fn compare(&self, s: &Shape) -> (f32, f32, f32) {
        let wider = (s.width - self.width) / self.width;
        let lifted = self.drop - s.drop;
        let longer = (s.length - self.length) / self.length;
        let smile = 0.6 * wider / 0.15 + 0.4 * lifted / 0.08;
        let surprise = 0.55 * longer / 0.10 - 0.25 * wider / 0.12 - 0.2 * lifted / 0.08;
        (smile, surprise, longer)
    }

    /// Moves the resting shape a little toward `s`, by how much `rate` says.
    fn learn(&mut self, s: &Shape, rate: f32) {
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
}

/// A sense of how the main face feels, kept on the camera thread only while
/// the user allows it. Holds the learned resting shape and the smoothed
/// reading; dropping it forgets both.
pub struct Feeling {
    reader: Option<Box<dyn ExpressionReader>>,
    neutral: Option<Neutral>,
    smoothed: Option<Expression>,
    last_seen: Option<Instant>,
    model_at: Option<Instant>,
    model: Option<(Reading, Instant)>,
}

impl Feeling {
    /// A Feeling that uses `reader` (if any) on top of the geometry.
    pub fn new(reader: Option<Box<dyn ExpressionReader>>) -> Feeling {
        Feeling {
            reader,
            neutral: None,
            smoothed: None,
            last_seen: None,
            model_at: None,
            model: None,
        }
    }

    /// A Feeling with the built-in model, or geometry alone if the model
    /// fails to load. Loading takes a moment, so call it on the camera thread.
    pub fn with_model() -> Feeling {
        match FerNet::new() {
            Ok(net) => Feeling::new(Some(Box::new(net))),
            Err(err) => {
                eprintln!("expression model failed to load, using geometry only: {err}");
                Feeling::new(None)
            }
        }
    }

    /// Reads the main (biggest, so first) face in `detections`, seen in
    /// `frame` at time `now`. `None` when there is no face or its landmarks
    /// make no sense. The frame is only borrowed and nothing of it is kept.
    pub fn feel(
        &mut self,
        frame: &RgbImage,
        detections: &[Detection],
        now: Instant,
    ) -> Option<Expression> {
        // `?` on an Option returns None from this function right away
        // when there's nothing there.
        let d = detections.first()?;
        let s = shape(d)?;
        // How long since the last face: for smoothing, and to start over
        // after the face has been gone a while.
        let dt = match self.last_seen {
            Some(t) if now.saturating_duration_since(t) < LOST_AFTER => {
                now.saturating_duration_since(t).as_secs_f32()
            }
            _ => {
                self.smoothed = None;
                0.0
            }
        };
        self.last_seen = Some(now);

        let facing = facing(&s);
        let score = smoothstep(0.5, 0.85, d.face.score as f32);
        // bbox width as a share of frame width: 0.03 is a face far across
        // a room, 0.1 and up is someone at the desk.
        let size = smoothstep(0.03, 0.10, d.bbox[2]);

        // The model, a few times a second, while the face is clear enough.
        if let Some(reader) = self.reader.as_mut() {
            let due = self
                .model_at
                .is_none_or(|t| now.saturating_duration_since(t) >= MODEL_EVERY);
            if due && facing > 0.4 && size > 0.3 {
                self.model_at = Some(now);
                if let Some(r) = reader.read(frame, d) {
                    self.model = Some((r, now));
                }
            }
        }
        let model = self
            .model
            .filter(|(_, t)| now.saturating_duration_since(*t) < MODEL_FRESH)
            .map(|(r, _)| r);

        // Compare with the resting shape, then let it learn from this frame.
        let neutral = self.neutral.get_or_insert(Neutral {
            width: s.width,
            drop: s.drop,
            length: s.length,
            samples: 0.0,
        });
        let (smile_geo, surprise_geo, longer) = neutral.compare(&s);
        if facing > 0.6 && score > 0.3 {
            // The model saying "neutral" teaches fast. "Less smiley than at
            // rest" (with the jaw closed, so not a gasp) means the rest was
            // learned mid-smile, so that corrects fast too. Calm frames
            // teach at the usual slow pace. Clear expressions barely count,
            // so a long grin doesn't become the new normal.
            let model_calm = model.is_some_and(|r| r.neutral > 0.6);
            let calm = smile_geo.abs() < 0.3 && surprise_geo.abs() < 0.3;
            let below = smile_geo < 0.0 && longer < 0.04;
            let tau = if model_calm || below {
                NEUTRAL_TAU_FAST
            } else if calm {
                NEUTRAL_TAU
            } else {
                NEUTRAL_TAU * 8.0
            };
            neutral.learn(&s, dt / tau);
        }
        let warm = (neutral.samples / WARM_SAMPLES).min(1.0);

        let mut smile = smile_geo.clamp(0.0, 1.0);
        let mut surprise = surprise_geo.clamp(0.0, 1.0);
        if let Some(r) = model {
            smile += (r.happy - smile) * MODEL_SMILE_WEIGHT;
            surprise += (r.surprised - surprise) * MODEL_SURPRISE_WEIGHT;
        }
        // Geometry alone is a rougher guess, so it is trusted a bit less.
        let source = if model.is_some() { 1.0 } else { 0.8 };
        let target = Expression {
            smile,
            surprise,
            facing,
            confidence: score * size * facing * warm * source,
        };

        // Ease toward the new reading, the same way however fast frames come.
        let k = 1.0 - (-dt / SMOOTH_TAU).exp();
        let next = match self.smoothed {
            None => target,
            Some(e) => Expression {
                smile: e.smile + (target.smile - e.smile) * k,
                surprise: e.surprise + (target.surprise - e.surprise) * k,
                facing: e.facing + (target.facing - e.facing) * k,
                confidence: e.confidence + (target.confidence - e.confidence) * k,
            },
        };
        self.smoothed = Some(next);
        Some(next)
    }
}

/// The expression model, built into the exe like the face detector.
static FER_MODEL: &[u8] =
    include_bytes!("../../assets/facial_expression_recognition_mobilefacenet_2022july.onnx");

/// The model's input is a 112x112 face, lined up so the five landmarks land
/// on these spots (the standard ArcFace layout, from OpenCV Zoo's demo).
const FER_SIZE: usize = 112;
const FER_POINTS: [[f32; 2]; 5] = [
    [38.2946, 51.6963],
    [73.5318, 51.5014],
    [56.0252, 71.7366],
    [41.5493, 92.3655],
    [70.7299, 92.2041],
];

/// The model's seven answers, in its order.
const FER_HAPPY: usize = 3;
const FER_NEUTRAL: usize = 4;
const FER_SURPRISED: usize = 6;
const FER_FEARFUL: usize = 2;

/// OpenCV Zoo's facial expression recogniser (Progressive Teacher on a
/// MobileFaceNet), run by tract like YuNet. Knows angry, disgust, fearful,
/// happy, neutral, sad and surprised.
pub struct FerNet {
    plan: Arc<TypedRunnableModel>,
}

impl FerNet {
    /// Loads and optimises the model (a fraction of a second in release).
    pub fn new() -> TractResult<FerNet> {
        let plan = tract_onnx::onnx()
            .model_for_read(&mut &FER_MODEL[..])?
            .with_input_fact(0, f32::fact([1, 3, FER_SIZE, FER_SIZE]).into())?
            .into_optimized()?
            .into_runnable()?;
        Ok(FerNet { plan })
    }

    fn run(&self, frame: &RgbImage, face: &Detection) -> TractResult<Reading> {
        let input = align(frame, &face.landmarks);
        let out = self.plan.run(tvec!(Tensor::from(input).into()))?;
        let logits = out[0].try_as_plain_ram()?.as_slice::<f32>()?.to_vec();
        let p = softmax(&logits);
        let at = |i: usize| p.get(i).copied().unwrap_or(0.0);
        Ok(Reading {
            happy: at(FER_HAPPY),
            // Wide eyes and an open mouth look much the same in fear.
            surprised: (at(FER_SURPRISED) + 0.5 * at(FER_FEARFUL)).min(1.0),
            neutral: at(FER_NEUTRAL),
        })
    }
}

impl ExpressionReader for FerNet {
    fn read(&mut self, frame: &RgbImage, face: &Detection) -> Option<Reading> {
        self.run(frame, face)
            .inspect_err(|err| eprintln!("expression model failed: {err}"))
            .ok()
    }
}

/// Turns raw scores into chances that add up to 1.
fn softmax(logits: &[f32]) -> Vec<f32> {
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let exps: Vec<f32> = logits.iter().map(|&l| (l - max).exp()).collect();
    let sum: f32 = exps.iter().sum();
    exps.iter().map(|e| e / sum).collect()
}

/// The similarity (scale, turn, shift) that best carries the model's five
/// spots onto the face's five landmarks, in frame pixels. Returns `(a, b,
/// from, to)`: a point `q` in the 112x112 picture comes from
/// `to + [[a, -b], [b, a]] * (q - from)` in the frame.
///
/// It is a least squares fit: with both point sets centred, `a` and `b`
/// come straight out of sums of dot and cross products.
fn similarity(frame_pts: &[[f32; 2]; 5]) -> (f32, f32, [f32; 2], [f32; 2]) {
    let centre = |pts: &[[f32; 2]; 5]| {
        let (sx, sy) = pts
            .iter()
            .fold((0.0, 0.0), |(x, y), p| (x + p[0], y + p[1]));
        [sx / 5.0, sy / 5.0]
    };
    let (from, to) = (centre(&FER_POINTS), centre(frame_pts));
    let (mut dot, mut cross, mut norm) = (0.0, 0.0, 0.0);
    for (t, f) in FER_POINTS.iter().zip(frame_pts) {
        let (tx, ty) = (t[0] - from[0], t[1] - from[1]);
        let (fx, fy) = (f[0] - to[0], f[1] - to[1]);
        dot += tx * fx + ty * fy;
        cross += tx * fy - ty * fx;
        norm += tx * tx + ty * ty;
    }
    (dot / norm, cross / norm, from, to)
}

/// Cuts the face out of the frame, upright and lined up the way the model
/// was trained, as the model's input: RGB planes scaled to -1..1.
/// `landmarks` are in 0..1 of the frame, as YuNet gives them.
fn align(frame: &RgbImage, landmarks: &[[f32; 2]; 5]) -> tract_ndarray::Array4<f32> {
    let (fw, fh) = frame.dimensions();
    let px = landmarks.map(|[x, y]| [x * fw as f32, y * fh as f32]);
    let (a, b, from, to) = similarity(&px);
    let raw = frame.as_raw();
    let mut input = tract_ndarray::Array4::from_elem((1, 3, FER_SIZE, FER_SIZE), -1.0f32);
    for oy in 0..FER_SIZE {
        for ox in 0..FER_SIZE {
            let (qx, qy) = (ox as f32 - from[0], oy as f32 - from[1]);
            let sx = to[0] + a * qx - b * qy;
            let sy = to[1] + b * qx + a * qy;
            // Bilinear: blend the four pixels around (sx, sy). Outside the
            // frame stays black (-1), as OpenCV's warpAffine leaves it.
            let (x0, y0) = (sx.floor(), sy.floor());
            if x0 < 0.0 || y0 < 0.0 || x0 + 1.0 >= fw as f32 || y0 + 1.0 >= fh as f32 {
                continue;
            }
            let (tx, ty) = (sx - x0, sy - y0);
            let (x0, y0) = (x0 as usize, y0 as usize);
            let at = |x: usize, y: usize, c: usize| raw[(y * fw as usize + x) * 3 + c] as f32;
            for c in 0..3 {
                let top = at(x0, y0, c) * (1.0 - tx) + at(x0 + 1, y0, c) * tx;
                let bottom = at(x0, y0 + 1, c) * (1.0 - tx) + at(x0 + 1, y0 + 1, c) * tx;
                let v = top * (1.0 - ty) + bottom * ty;
                input[[0, c, oy, ox]] = v / 127.5 - 1.0;
            }
        }
    }
    input
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mind::Face;

    /// A made-up face for the tests, described in its own frame (units of
    /// the distance between the eyes, eyes at y = 0): mouth `width`, mouth
    /// corners `mouth_y` below the eyes, face box `length`, nose `yaw`
    /// sideways, head tilted by `roll` radians.
    #[derive(Clone, Copy)]
    struct Fake {
        width: f32,
        mouth_y: f32,
        length: f32,
        yaw: f32,
        roll: f32,
        score: f32,
    }

    const REST: Fake = Fake {
        width: 0.83,
        mouth_y: 1.16,
        length: 2.6,
        yaw: 0.0,
        roll: 0.0,
        score: 0.9,
    };

    fn detection(f: Fake) -> Detection {
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

    /// Feeds `frames` copies of `face` at 15 Hz from `*t`; returns the last reading.
    fn watch(feeling: &mut Feeling, t: &mut Instant, face: Fake, frames: usize) -> Expression {
        let frame = RgbImage::new(1, 1);
        let mut last = None;
        for _ in 0..frames {
            *t += Duration::from_millis(66);
            last = feeling.feel(&frame, &[detection(face)], *t);
        }
        last.expect("a reading")
    }

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

    #[test]
    fn a_smile_reads_as_a_smile_after_learning_the_resting_face() {
        let (mut feeling, mut t) = (Feeling::new(None), Instant::now());
        let rest = watch(&mut feeling, &mut t, REST, 45);
        assert!(rest.smile < 0.05 && rest.surprise < 0.05, "{rest:?}");
        assert!(rest.facing > 0.95 && rest.confidence > 0.5, "{rest:?}");
        // Corners 18% wider and lifted.
        let grin = Fake {
            width: 0.98,
            mouth_y: 1.08,
            ..REST
        };
        let smiling = watch(&mut feeling, &mut t, grin, 15);
        assert!(smiling.smile > 0.7, "{smiling:?}");
        assert!(smiling.surprise < 0.1, "{smiling:?}");
    }

    #[test]
    fn a_naturally_wide_mouth_is_not_a_smile() {
        let (mut feeling, mut t) = (Feeling::new(None), Instant::now());
        let wide = Fake {
            width: 1.0,
            mouth_y: 1.1,
            ..REST
        };
        let rest = watch(&mut feeling, &mut t, wide, 45);
        assert!(rest.smile < 0.05, "{rest:?}");
    }

    #[test]
    fn a_rest_learned_mid_smile_corrects_itself() {
        let (mut feeling, mut t) = (Feeling::new(None), Instant::now());
        let grin = Fake {
            width: 0.98,
            mouth_y: 1.08,
            ..REST
        };
        watch(&mut feeling, &mut t, grin, 20);
        // Relaxing for a while teaches it the real resting face...
        watch(&mut feeling, &mut t, REST, 15 * 20);
        // ...so the same grin reads as a smile again.
        let smiling = watch(&mut feeling, &mut t, grin, 15);
        assert!(smiling.smile > 0.5, "{smiling:?}");
    }

    #[test]
    fn a_dropped_jaw_reads_as_surprise() {
        let (mut feeling, mut t) = (Feeling::new(None), Instant::now());
        watch(&mut feeling, &mut t, REST, 45);
        let gasp = Fake {
            width: 0.72,
            mouth_y: 1.22,
            length: 2.95,
            ..REST
        };
        let startled = watch(&mut feeling, &mut t, gasp, 15);
        assert!(startled.surprise > 0.7, "{startled:?}");
        assert!(startled.smile < 0.05, "{startled:?}");
    }

    #[test]
    fn a_turned_face_faces_less_and_counts_less() {
        let (mut feeling, mut t) = (Feeling::new(None), Instant::now());
        let straight = watch(&mut feeling, &mut t, REST, 45);
        let turned = watch(&mut feeling, &mut t, Fake { yaw: 0.4, ..REST }, 30);
        assert!(turned.facing < 0.2, "{turned:?}");
        assert!(turned.confidence < straight.confidence * 0.3, "{turned:?}");
        // So does a head tipped right over.
        let tipped = watch(&mut feeling, &mut t, Fake { roll: 0.9, ..REST }, 30);
        assert!(tipped.facing < 0.1, "{tipped:?}");
    }

    #[test]
    fn an_unsure_detection_counts_less() {
        let (mut sure, mut t) = (Feeling::new(None), Instant::now());
        let sure = watch(&mut sure, &mut t, REST, 45);
        let (mut unsure, mut t) = (Feeling::new(None), Instant::now());
        let unsure = watch(&mut unsure, &mut t, Fake { score: 0.6, ..REST }, 45);
        assert!(
            unsure.confidence < sure.confidence * 0.5,
            "{unsure:?} {sure:?}"
        );
    }

    #[test]
    fn no_face_no_expression() {
        let mut feeling = Feeling::new(None);
        assert_eq!(
            feeling.feel(&RgbImage::new(1, 1), &[], Instant::now()),
            None
        );
    }

    /// A reader that always says the same thing and counts its calls. The
    /// count is shared (`Arc`) so the test can still see it after the reader
    /// has been boxed up inside the Feeling.
    struct Says(Reading, Arc<std::sync::atomic::AtomicUsize>);

    impl ExpressionReader for Says {
        fn read(&mut self, _: &RgbImage, _: &Detection) -> Option<Reading> {
            self.1.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Some(self.0)
        }
    }

    #[test]
    fn the_model_is_blended_in_a_few_times_a_second() {
        let happy = Reading {
            happy: 1.0,
            surprised: 0.0,
            neutral: 0.0,
        };
        let runs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut feeling = Feeling::new(Some(Box::new(Says(happy, Arc::clone(&runs)))));
        let mut t = Instant::now();
        // A resting face the model calls happy: the model wins most of it.
        let e = watch(&mut feeling, &mut t, REST, 30);
        assert!(e.smile > 0.5 && e.smile < 0.7, "{e:?}");
        // 30 frames at 15 Hz is 2 s: about 8 model runs, not 30.
        let runs = runs.load(std::sync::atomic::Ordering::Relaxed);
        assert!((7..=10).contains(&runs), "{runs} runs");
    }

    #[test]
    fn landmarks_on_the_template_line_up_exactly() {
        // A face twice the template's size, shifted: scale 0.5, no turn.
        let pts = FER_POINTS.map(|[x, y]| [x * 2.0 + 100.0, y * 2.0 + 50.0]);
        let (a, b, from, to) = similarity(&pts);
        assert!((a - 2.0).abs() < 1e-4 && b.abs() < 1e-4, "{a} {b}");
        for (t, f) in FER_POINTS.iter().zip(&pts) {
            let (qx, qy) = (t[0] - from[0], t[1] - from[1]);
            assert!((to[0] + a * qx - b * qy - f[0]).abs() < 1e-3);
            assert!((to[1] + b * qx + a * qy - f[1]).abs() < 1e-3);
        }
    }

    #[test]
    fn the_expression_model_loads_and_reads_a_face() {
        // A synthetic "face": a light oval with dark eyes and mouth, in a
        // 640x360 frame. Not a real face; this checks the plumbing.
        let mut frame = RgbImage::from_pixel(640, 360, image::Rgb([40, 40, 50]));
        let (cx, cy) = (320.0f32, 180.0f32);
        for (x, y, px) in frame.enumerate_pixels_mut() {
            let (dx, dy) = ((x as f32 - cx) / 70.0, (y as f32 - cy) / 95.0);
            if dx * dx + dy * dy < 1.0 {
                *px = image::Rgb([210, 170, 150]);
            }
        }
        let lm_px = [
            [295.0, 160.0],
            [345.0, 160.0],
            [320.0, 190.0],
            [298.0, 220.0],
            [342.0, 220.0],
        ];
        for [x, y] in lm_px {
            for dy in -4i32..=4 {
                for dx in -6i32..=6 {
                    frame.put_pixel(
                        (x as i32 + dx) as u32,
                        (y as i32 + dy) as u32,
                        image::Rgb([30, 20, 20]),
                    );
                }
            }
        }
        let mut d = detection(REST);
        d.landmarks = lm_px.map(|[x, y]| [x / 640.0, y / 360.0]);
        let mut net = FerNet::new().expect("model loads");
        let r = net.read(&frame, &d).expect("model runs");
        for p in [r.happy, r.surprised, r.neutral] {
            assert!(p.is_finite() && (0.0..=1.0).contains(&p), "{r:?}");
        }
        assert!(r.happy + r.neutral <= 1.0 + 1e-4, "{r:?}");
    }

    /// How long one model reading takes, lining up included. Only means
    /// something optimised: `cargo test --release -- --ignored model_cost --nocapture`.
    #[test]
    #[ignore]
    fn model_cost() {
        let frame = RgbImage::from_pixel(1280, 720, image::Rgb([120, 100, 90]));
        let mut d = detection(REST);
        d.landmarks =
            FER_POINTS.map(|[x, y]| [(x * 2.0 + 500.0) / 1280.0, (y * 2.0 + 200.0) / 720.0]);
        let mut net = FerNet::new().unwrap();
        net.read(&frame, &d);
        let t = Instant::now();
        let n = 50;
        for _ in 0..n {
            net.read(&frame, &d);
        }
        let each = t.elapsed() / n;
        let t = Instant::now();
        for _ in 0..n {
            std::hint::black_box(align(&frame, &d.landmarks));
        }
        let lining_up = t.elapsed() / n;
        // The face detector on the same frame, to compare.
        use super::super::Detector;
        let mut yunet = super::super::face::YuNet::new().unwrap();
        let t = Instant::now();
        for _ in 0..n {
            yunet.detect(&frame);
        }
        eprintln!(
            "model reading: {each:?}, of which lining up {lining_up:?}; face detector: {:?}",
            t.elapsed() / n
        );
    }
}
