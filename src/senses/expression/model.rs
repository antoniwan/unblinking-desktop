//! The second reader: OpenCV Zoo's facial expression model, run by tract.
//! It cuts the face out of the frame, lines it up the way the model was
//! trained, and turns the model's seven answers into a [`Reading`].

use std::sync::Arc;

use image::RgbImage;
use tract_onnx::prelude::*;

use super::{ExpressionReader, Reading};
use crate::senses::Detection;

/// The expression model, built into the exe like the face detector.
/// `concat!` and `env!` run at compile time, so the path is the crate's own
/// folder however deep this file sits.
static FER_MODEL: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/facial_expression_recognition_mobilefacenet_2022july.onnx"
));

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
    use std::time::Instant;

    use super::super::fake_face::{REST, detection};
    use super::*;

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
        use crate::senses::Detector;
        let mut yunet = crate::senses::face::YuNet::new().unwrap();
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
