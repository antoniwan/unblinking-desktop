//! Expression: a rough, on-device reading of the face the eye is watching.
//! Only while the user has opted in. Nothing is stored or sent.
//!
//! Two readers work together, in layers:
//!
//! 1. **Geometry** (`geometry`) always runs. YuNet already gives five
//!    landmarks (eyes, nose tip, mouth corners). A smile pulls the mouth
//!    corners apart and up; a dropped jaw makes the face box longer and the
//!    mouth narrower. Faces differ a lot, so instead of fixed thresholds we
//!    compare against *this* face's own resting shape, learned while the
//!    session runs.
//! 2. **A small model** (`model`: OpenCV Zoo's facial expression recogniser,
//!    a MobileFaceNet, Apache 2.0) looks at the face itself about 4 times a
//!    second and says how happy, surprised or neutral it looks. Its answer
//!    is blended over the geometry and also tells the geometry when the
//!    face is at rest, so it learns the resting shape faster.
//!
//! Everything lives in a [`Feeling`], which the camera thread keeps only
//! while the user has said yes. Turning it off drops the Feeling, and with it
//! everything learned about the face. Frames are only borrowed, never kept.
//! This file holds the Feeling and the blend of the two readers.

// `#[cfg(test)]` leaves `fake_face` out of the real build: only the tests
// in `geometry`, `model` and here use it.
#[cfg(test)]
mod fake_face;
mod geometry;
mod model;

use std::time::{Duration, Instant};

use image::RgbImage;

use super::Detection;
use geometry::{NEUTRAL_TAU, NEUTRAL_TAU_FAST, Neutral, facing, shape, smoothstep};
// `pub use` re-exports: the model is reachable as
// `crate::senses::expression::FerNet`, without naming the file it lives in.
pub use model::FerNet;

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
/// After this long without a face, the smoothed reading starts over.
const LOST_AFTER: Duration = Duration::from_secs(1);

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
        let neutral = self.neutral.get_or_insert(Neutral::new(&s));
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
        let warm = neutral.warmth();

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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::fake_face::{Fake, REST, detection};
    use super::*;

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
}
