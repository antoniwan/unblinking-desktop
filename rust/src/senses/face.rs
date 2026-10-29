//! The face detector: YuNet (OpenCV's small ONNX face model, MIT licensed),
//! run on the CPU by tract, a pure Rust inference engine.
//!
//! MediaPipe used to turn the model's raw output into boxes for us; here
//! `decode` and `nms` do that by hand, following OpenCV's FaceDetectorYN.

use std::sync::Arc;

use image::RgbImage;
use image::imageops::{self, FilterType};
use tract_onnx::prelude::*;

use super::{Detection, Detector};
use crate::mind::Face;

/// The model, built into the exe so there is nothing to download at setup.
static MODEL: &[u8] = include_bytes!("../../assets/face_detection_yunet_2023mar.onnx");

/// The size frames are shrunk to before the model sees them. Both sides must
/// be multiples of 32 (the model's coarsest stride); 320 wide is about 4x
/// faster than 640 and still finds a face across a desk.
const IN_W: u32 = 320;
const IN_H: u32 = 256;
const STRIDES: [usize; 3] = [8, 16, 32];
/// Keep boxes at least this sure (MediaPipe's minDetectionConfidence).
const MIN_SCORE: f32 = 0.5;
/// Boxes overlapping more than this are the same face.
const NMS_IOU: f32 = 0.3;

/// A face box in model input pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Bbox {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    score: f32,
    /// Right eye, left eye, nose tip, right and left mouth corners.
    kps: [[f32; 2]; 5],
}

pub struct YuNet {
    plan: Arc<TypedRunnableModel>,
}

impl YuNet {
    /// Loads and optimises the model. Takes about a second, so the camera
    /// thread does it, not the window.
    pub fn new() -> TractResult<YuNet> {
        let plan = tract_onnx::onnx()
            // The file records shapes for a 640x640 input; forget them so
            // tract works them out again for our smaller one.
            .with_ignore_value_info(true)
            .model_for_read(&mut &MODEL[..])?
            .with_input_fact(0, f32::fact([1, 3, IN_H as usize, IN_W as usize]).into())?
            .into_optimized()?
            .into_runnable()?;
        Ok(YuNet { plan })
    }

    fn run(&self, frame: &RgbImage) -> TractResult<Vec<Detection>> {
        let (fw, fh) = frame.dimensions();
        // Shrink to fit IN_W x IN_H without stretching; the rest stays black.
        let k = (IN_W as f32 / fw as f32).min(IN_H as f32 / fh as f32);
        let (sw, sh) = (
            ((fw as f32 * k) as u32).max(1),
            ((fh as f32 * k) as u32).max(1),
        );
        let small = imageops::resize(frame, sw, sh, FilterType::Triangle);

        // The model wants planes of blue, green, red (OpenCV's order), 0..255.
        let input = tract_ndarray::Array4::from_shape_fn(
            (1, 3, IN_H as usize, IN_W as usize),
            |(_, c, y, x)| match small.get_pixel_checked(x as u32, y as u32) {
                Some(px) => px[2 - c] as f32,
                None => 0.0,
            },
        );
        let out = self.plan.run(tvec!(Tensor::from(input).into()))?;

        // Outputs come as cls_8, cls_16, cls_32, obj_*, bbox_*, kps_*.
        let mut boxes = Vec::new();
        for (i, &stride) in STRIDES.iter().enumerate() {
            let cls = out[i].try_as_plain_ram()?.as_slice::<f32>()?;
            let obj = out[3 + i].try_as_plain_ram()?.as_slice::<f32>()?;
            let bbox = out[6 + i].try_as_plain_ram()?.as_slice::<f32>()?;
            let kps = out[9 + i].try_as_plain_ram()?.as_slice::<f32>()?;
            boxes.extend(decode(stride, cls, obj, bbox, kps));
        }
        let (sw, sh) = (sw as f32, sh as f32);
        let faces = nms(boxes)
            .into_iter()
            .map(|b| {
                let kps = b.kps.map(|[x, y]| [x / sw, y / sh]);
                Detection {
                    // The eye looks at your eyes, not the middle of the box (your nose).
                    face: Face {
                        x: ((kps[0][0] + kps[1][0]) / 2.0) as f64,
                        y: ((kps[0][1] + kps[1][1]) / 2.0) as f64,
                        size: (b.w / sw) as f64,
                        score: b.score as f64,
                        aspect: fw as f64 / fh as f64,
                    },
                    bbox: [b.x / sw, b.y / sh, b.w / sw, b.h / sh],
                    landmarks: kps,
                }
            })
            .collect();
        Ok(faces)
    }
}

impl Detector for YuNet {
    fn detect(&mut self, frame: &RgbImage) -> Vec<Detection> {
        // A failed run sees nothing this frame; the next frame tries again.
        self.run(frame).unwrap_or_else(|err| {
            eprintln!("face detector failed: {err}");
            vec![]
        })
    }
}

/// Turns one stride's grid of raw outputs into scored boxes.
/// Each cell predicts a center offset in cells and a log-scaled size, plus
/// five landmarks (right eye, left eye, nose, mouth corners) as offsets.
fn decode(stride: usize, cls: &[f32], obj: &[f32], bbox: &[f32], kps: &[f32]) -> Vec<Bbox> {
    let cols = IN_W as usize / stride;
    let s = stride as f32;
    let mut boxes = Vec::new();
    for (idx, (&c, &o)) in cls.iter().zip(obj).enumerate() {
        let score = (c.clamp(0.0, 1.0) * o.clamp(0.0, 1.0)).sqrt();
        if score < MIN_SCORE {
            continue;
        }
        let (row, col) = ((idx / cols) as f32, (idx % cols) as f32);
        let b = &bbox[idx * 4..idx * 4 + 4];
        let (cx, cy) = ((col + b[0]) * s, (row + b[1]) * s);
        let (w, h) = (b[2].exp() * s, b[3].exp() * s);
        let k = &kps[idx * 10..idx * 10 + 10];
        // `std::array::from_fn` builds the five points from their index.
        let kps = std::array::from_fn(|j| [(col + k[2 * j]) * s, (row + k[2 * j + 1]) * s]);
        boxes.push(Bbox {
            x: cx - w / 2.0,
            y: cy - h / 2.0,
            w,
            h,
            score,
            kps,
        });
    }
    boxes
}

/// Non-maximum suppression: keep the surest box, drop the ones that overlap
/// it, repeat. Returns the survivors biggest first, as senses.js sorted them.
fn nms(mut boxes: Vec<Bbox>) -> Vec<Bbox> {
    boxes.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<Bbox> = Vec::new();
    for b in boxes {
        if kept.iter().all(|k| iou(k, &b) <= NMS_IOU) {
            kept.push(b);
        }
    }
    kept.sort_by(|a, b| b.w.total_cmp(&a.w));
    kept
}

/// How much two boxes overlap: shared area over combined area, 0..1.
fn iou(a: &Bbox, b: &Bbox) -> f32 {
    let w = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
    let h = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
    if w <= 0.0 || h <= 0.0 {
        return 0.0;
    }
    let shared = w * h;
    shared / (a.w * a.h + b.w * b.h - shared)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bbox(x: f32, y: f32, w: f32, score: f32) -> Bbox {
        Bbox {
            x,
            y,
            w,
            h: w,
            score,
            kps: [[x + w / 2.0, y + w / 3.0]; 5],
        }
    }

    #[test]
    fn overlapping_boxes_collapse_to_the_surest() {
        let kept = nms(vec![
            bbox(0.0, 0.0, 10.0, 0.6),
            bbox(1.0, 1.0, 10.0, 0.9),
            bbox(50.0, 0.0, 20.0, 0.7),
        ]);
        // The far box is bigger, so it comes first; the 0.6 one is gone.
        assert_eq!(
            kept,
            vec![bbox(50.0, 0.0, 20.0, 0.7), bbox(1.0, 1.0, 10.0, 0.9)]
        );
    }

    #[test]
    fn decode_places_a_box_in_its_cell() {
        // Stride 32 on a 320-wide input: 10 columns. Cell 12 is row 1, col 2.
        let n = (IN_W as usize / 32) * (IN_H as usize / 32);
        let (mut cls, mut obj, mut bbox) = (vec![0.0; n], vec![0.0; n], vec![0.0; n * 4]);
        let mut kps = vec![0.0; n * 10];
        cls[12] = 1.0;
        obj[12] = 1.0;
        bbox[48..52].copy_from_slice(&[0.5, 0.5, 0.0, 0.0]); // centered, one cell wide
        kps[120..124].copy_from_slice(&[0.25, 0.25, 0.75, 0.25]); // eyes in the top quarter
        let boxes = decode(32, &cls, &obj, &bbox, &kps);
        let kps = boxes[0].kps;
        assert_eq!(
            boxes,
            vec![Bbox {
                x: 64.0,
                y: 32.0,
                w: 32.0,
                h: 32.0,
                score: 1.0,
                kps,
            }]
        );
        assert_eq!(kps[0], [72.0, 40.0]);
        assert_eq!(kps[1], [88.0, 40.0],);
    }

    #[test]
    fn a_blank_frame_has_no_faces() {
        let mut net = YuNet::new().expect("model loads");
        assert!(net.detect(&RgbImage::new(640, 480)).is_empty());
    }

    /// Needs a real camera and a face in front of it: `cargo test -- --ignored`.
    /// Mirroring the frame must mirror the face, so x and its mirror's x add
    /// up to 1. Nothing is saved; the frame lives only in memory.
    #[test]
    #[ignore]
    fn a_mirrored_frame_mirrors_the_face() {
        use nokhwa::pixel_format::RgbFormat;
        // The same camera settings the app uses.
        let mut camera = super::super::camera::open_camera().unwrap();
        let mut frame = None;
        for _ in 0..30 {
            frame = Some(camera.frame().unwrap());
        }
        camera.stop_stream().unwrap();
        let rgb = frame.unwrap().decode_image::<RgbFormat>().unwrap();
        let mut net = YuNet::new().unwrap();
        let faces: Vec<Face> = net.detect(&rgb).iter().map(|d| d.face).collect();
        let mirrored: Vec<Face> = net
            .detect(&imageops::flip_horizontal(&rgb))
            .iter()
            .map(|d| d.face)
            .collect();
        eprintln!(
            "frame {:?}
faces {faces:?}
mirrored {mirrored:?}",
            rgb.dimensions()
        );
        let (a, b) = (
            faces.first().expect("a face"),
            mirrored.first().expect("a mirrored face"),
        );
        assert!(
            (a.x + b.x - 1.0).abs() < 0.05,
            "x {} + {} should be 1",
            a.x,
            b.x
        );
        assert!((a.y - b.y).abs() < 0.05 && (a.size - b.size).abs() < 0.05);
    }
}
