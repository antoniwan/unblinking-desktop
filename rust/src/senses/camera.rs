//! The camera runs on its own thread, so a slow frame never stalls the eye.
//! It looks for faces at most 15 times a second (about 3 while the eye is
//! hidden) and sends what it saw to the main loop over a channel.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use nokhwa::Camera;
use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{
    CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType, Resolution,
};

use image::RgbImage;
use image::imageops::{self, FilterType};

use super::{Detection, Detector};

/// Face detection is the costly part; 15 times a second is plenty.
const LOOK_EVERY: Duration = Duration::from_millis(66);
/// While the eye is hidden nobody sees it react, so look about 3 times a
/// second: a fifth of the work. The camera stays open (reopening one takes a
/// second or more, and its light would blink), and a fresh sight is ready the
/// moment the eye comes back.
const LOOK_EVERY_RESTING: Duration = Duration::from_millis(333);

/// How wide the live picture for Blinki's Senses is.
const PREVIEW_WIDTH: u32 = 480;

/// One report from the camera thread.
#[derive(Clone, Debug, Default)]
pub struct Sight {
    pub detections: Vec<Detection>,
    /// True while a camera is open and its frames are being looked at.
    pub can_see: bool,
    /// A small copy of the frame, only while Blinki's Senses is open. It is
    /// shown live and dropped; nothing is ever written to disk.
    pub preview: Option<Arc<RgbImage>>,
}

/// The open eyes: a handle on the camera thread.
/// Dropping it stops the thread and lets go of the camera.
pub struct Eyes {
    // `Arc<AtomicBool>` is a flag both threads can see. An atomic can be
    // read and written from several threads at once without a lock;
    // the Arc lets both threads own it.
    stop: Arc<AtomicBool>,
    /// Set while Blinki's Senses is open and wants the picture.
    preview: Arc<AtomicBool>,
    /// Set while the eye is hidden: look at faces less often.
    resting: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    reports: Receiver<Sight>,
    latest: Sight,
}

impl Eyes {
    /// Starts a thread that builds a detector with `make`, then opens the
    /// first camera and starts looking.
    ///
    /// `make` is a closure, so slow set-up (loading the face model) happens
    /// on the camera thread, not in the window. Its `Box<dyn Detector>` is
    /// "some type that implements Detector", kept on the heap; the camera
    /// code never needs to know which one. The bounds say `make` can be
    /// sent to another thread (`Send`) and borrows nothing short-lived
    /// (`'static`), since that thread may outlive this call.
    pub fn open<F>(make: F) -> Eyes
    where
        F: FnOnce() -> Result<Box<dyn Detector>, String> + Send + 'static,
    {
        let stop = Arc::new(AtomicBool::new(false));
        // A channel is a one-way queue between threads: the camera thread
        // keeps the Sender, we keep the Receiver.
        let (tx, reports) = mpsc::channel();
        let preview = Arc::new(AtomicBool::new(false));
        let resting = Arc::new(AtomicBool::new(false));
        let (flag, want, rest) = (
            Arc::clone(&stop),
            Arc::clone(&preview),
            Arc::clone(&resting),
        );
        // `move` hands the closure ownership of what it uses (the detector,
        // the flags, the sender), so they travel to the new thread with it.
        let thread = thread::Builder::new()
            .name("camera".into())
            .spawn(move || watch(make, flag, want, rest, tx))
            .expect("spawn camera thread");
        Eyes {
            stop,
            preview,
            resting,
            thread: Some(thread),
            reports,
            latest: Sight::default(),
        }
    }

    /// The most recent report, after taking in any that arrived since the last call.
    pub fn look(&mut self) -> &Sight {
        // `try_recv` never waits: it returns Err once the queue is empty.
        while let Ok(sight) = self.reports.try_recv() {
            self.latest = sight;
        }
        &self.latest
    }

    /// Asks the camera thread to include (or stop including) a small picture.
    pub fn set_preview(&mut self, on: bool) {
        self.preview.store(on, Ordering::Relaxed);
        if !on {
            self.latest.preview = None;
        }
    }

    /// Asks the camera thread to look less often (while the eye is hidden),
    /// or at the full rate again. Cheap enough to call every loop.
    pub fn set_resting(&self, on: bool) {
        self.resting.store(on, Ordering::Relaxed);
    }
}

/// `Drop` is Rust's destructor: it runs when a value goes away (goes out of
/// scope, or an `Option` holding it is set to `None`). Here it stops the
/// thread and waits until the camera is really off, however the Eyes end.
impl Drop for Eyes {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub(super) fn open_camera() -> Result<Camera, nokhwa::NokhwaError> {
    // 1280x720: webcams usually give their whole field of view in 16:9, while
    // 640x480 is often a tighter crop (it was on the eMeet C960), which
    // throws off where the eye thinks you are. MJPEG is what most webcams
    // send fastest; nokhwa picks the closest format the camera has.
    let wanted = CameraFormat::new(Resolution::new(1280, 720), FrameFormat::MJPEG, 30);
    let format = RequestedFormat::new::<RgbFormat>(RequestedFormatType::Closest(wanted));
    let mut camera = Camera::new(CameraIndex::Index(0), format)?;
    camera.open_stream()?;
    Ok(camera)
}

/// The camera thread's whole life.
fn watch<F>(
    make: F,
    stop: Arc<AtomicBool>,
    preview: Arc<AtomicBool>,
    resting: Arc<AtomicBool>,
    tx: Sender<Sight>,
) where
    F: FnOnce() -> Result<Box<dyn Detector>, String>,
{
    // A camera without a detector sees nothing, so don't turn its light on.
    let mut detector = match make() {
        Ok(detector) => detector,
        Err(err) => {
            eprintln!("face detector failed to load: {err}");
            let _ = tx.send(Sight::default());
            return;
        }
    };
    let mut camera = match open_camera() {
        Ok(camera) => camera,
        Err(err) => {
            eprintln!("no sight: {err}");
            let _ = tx.send(Sight::default());
            return;
        }
    };
    let f = camera.camera_format();
    eprintln!(
        "sight: {} {}x{} at {} fps",
        f.format(),
        f.width(),
        f.height(),
        f.frame_rate()
    );
    let _ = tx.send(Sight {
        can_see: true,
        ..Sight::default()
    });

    let mut last_look: Option<Instant> = None;
    while !stop.load(Ordering::Relaxed) {
        // Waits for the next frame. Reading every frame keeps the camera's
        // queue short, so the one we do look at is fresh.
        let frame = match camera.frame() {
            Ok(frame) => frame,
            Err(err) => {
                eprintln!("sight lost: {err}");
                break;
            }
        };
        // Skipped frames are never decoded, so they cost almost nothing.
        let every = if resting.load(Ordering::Relaxed) {
            LOOK_EVERY_RESTING
        } else {
            LOOK_EVERY
        };
        if last_look.is_some_and(|t| t.elapsed() < every) {
            continue;
        }
        last_look = Some(Instant::now());
        let rgb = match frame.decode_image::<RgbFormat>() {
            Ok(rgb) => rgb,
            Err(err) => {
                eprintln!("bad camera frame: {err}");
                continue;
            }
        };
        let detections = detector.detect(&rgb);
        let preview = preview.load(Ordering::Relaxed).then(|| {
            let h = rgb.height() * PREVIEW_WIDTH / rgb.width().max(1);
            Arc::new(imageops::resize(
                &rgb,
                PREVIEW_WIDTH,
                h,
                FilterType::Triangle,
            ))
        });
        // A send fails only when the Eyes are gone; then there's no one to tell.
        if tx
            .send(Sight {
                detections,
                can_see: true,
                preview,
            })
            .is_err()
        {
            break;
        }
    }
    let _ = camera.stop_stream();
    let _ = tx.send(Sight::default());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A detector that finds nothing but remembers each frame's size.
    struct Counter(Arc<Mutex<Vec<(u32, u32)>>>);

    impl Detector for Counter {
        fn detect(&mut self, frame: &image::RgbImage) -> Vec<Detection> {
            self.0.lock().unwrap().push(frame.dimensions());
            vec![]
        }
    }

    /// Needs a real camera, so it only runs with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn the_camera_delivers_frames_and_lets_go() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let counter = Counter(Arc::clone(&seen));
        let mut eyes = Eyes::open(move || Ok(Box::new(counter) as Box<dyn Detector>));
        thread::sleep(Duration::from_secs(3));
        assert!(eyes.look().can_see, "camera did not open");
        drop(eyes); // closes the camera
        let seen = seen.lock().unwrap();
        eprintln!("looked at {} frames of {:?}", seen.len(), seen.first());
        // About 15 looks a second, after the camera warms up.
        assert!(seen.len() >= 10, "only {} frames", seen.len());
    }
}
