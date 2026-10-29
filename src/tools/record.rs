//! `--record=FILE.gif`: keeps the frames the app really draws (the eye and
//! its whispers, as they reach the screen) and writes them out as an
//! animated GIF over a dark backdrop. Screen capture tools miss frames and
//! can show stale text; this can't.

use std::error::Error;
use std::time::Instant;

/// The backdrop the frames are laid over (the Blinki Studio's dark paper).
const BACKDROP: [u8; 3] = [0x0d, 0x0b, 0x10];
/// Keep at most this many frames a second; plenty for a smooth GIF.
const FPS: f32 = 25.0;

pub struct Recording {
    path: String,
    started: Instant,
    size: (u32, u32),
    /// Each frame's time (seconds) and premultiplied RGBA pixels.
    frames: Vec<(f32, Vec<u8>)>,
}

impl Recording {
    pub fn new(path: String) -> Recording {
        Recording {
            path,
            started: Instant::now(),
            size: (0, 0),
            frames: Vec::new(),
        }
    }

    /// Whether it's time for another frame.
    pub fn wants_frame(&self) -> bool {
        let t = self.started.elapsed().as_secs_f32();
        self.frames
            .last()
            .is_none_or(|(last, _)| t - last >= 1.0 / FPS)
    }

    pub fn add(&mut self, size: (u32, u32), rgba: Vec<u8>) {
        self.size = size;
        self.frames
            .push((self.started.elapsed().as_secs_f32(), rgba));
    }

    /// Writes the GIF, each frame lasting until the next one was taken.
    pub fn finish(self) -> Result<(), Box<dyn Error>> {
        let (w, h) = self.size;
        let file = std::io::BufWriter::new(std::fs::File::create(&self.path)?);
        let mut gif = gif::Encoder::new(file, w as u16, h as u16, &[])?;
        gif.set_repeat(gif::Repeat::Infinite)?;
        for (i, (t, rgba)) in self.frames.iter().enumerate() {
            let next = self.frames.get(i + 1).map_or(t + 1.0 / FPS, |(n, _)| *n);
            let rgb = over_backdrop(rgba);
            // speed 10: NeuQuant's quick setting, finds a palette per frame.
            let mut frame = gif::Frame::from_rgb_speed(w as u16, h as u16, &rgb, 10);
            frame.delay = ((next - t) * 100.0).round().max(2.0) as u16; // in 1/100 s
            gif.write_frame(&frame)?;
        }
        eprintln!(
            "{} written: {} frames, {}x{}",
            self.path,
            self.frames.len(),
            w,
            h
        );
        Ok(())
    }
}

/// Premultiplied RGBA over the backdrop: colour + backdrop * (1 - alpha).
fn over_backdrop(rgba: &[u8]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    for px in rgba.as_chunks::<4>().0 {
        let keep = 255 - px[3] as u32;
        for i in 0..3 {
            rgb.push((px[i] as u32 + (BACKDROP[i] as u32 * keep + 127) / 255).min(255) as u8);
        }
    }
    rgb
}
