//! `--demo`: a short scripted performance for screenshots and the README
//! GIF. No camera, no mic: the cursor, the sounds it hears, the moods and
//! the whispers follow a timeline, while the mind reacts to them as usual.
//! So the gaze, the flinch and the mood blends are the real thing.

use std::time::Instant;

use crate::desktop::Point;

/// How long the performance runs before the eye closes.
pub const LENGTH: f32 = 14.0;

/// When each mood takes over (seconds from the start).
const MOODS: [(f32, &str); 6] = [
    (0.0, "Watchful"),
    (3.2, "Curious"),
    (5.8, "Wrathful"),
    (8.0, "Delighted"),
    (10.2, "Lonely"),
    (12.2, "Drowsy"),
];

/// What it whispers, and when.
const WHISPERS: [(f32, &str); 5] = [
    (1.3, "I see you."),
    (3.6, "What is that?"),
    (6.0, "Silence."),
    (8.3, "There you are."),
    (10.6, "Where are you?"),
];

/// A loud bang, to make it flinch.
const BANG: (f32, f32) = (5.8, 5.9);

pub struct Demo {
    started: Instant,
    whispered: usize,
}

impl Demo {
    pub fn new() -> Demo {
        Demo {
            started: Instant::now(),
            whispered: 0,
        }
    }

    fn t(&self) -> f32 {
        self.started.elapsed().as_secs_f32()
    }

    /// The mood the script wants now.
    pub fn mood(&self) -> &'static str {
        let t = self.t();
        MOODS
            .iter()
            .rev()
            .find(|(at, _)| t >= *at)
            .map_or("Watchful", |(_, m)| m)
    }

    /// The loudness it "hears": a murmur, and one bang.
    pub fn level(&self) -> f64 {
        let t = self.t();
        if (BANG.0..BANG.1).contains(&t) {
            0.95
        } else {
            0.06 + 0.04 * (t * 3.1).sin().abs() as f64
        }
    }

    /// A whisper that has just come due, if any.
    pub fn whisper(&mut self) -> Option<&'static str> {
        let (at, text) = *WHISPERS.get(self.whispered)?;
        if self.t() < at {
            return None;
        }
        self.whispered += 1;
        Some(text)
    }

    /// Where the "cursor" is: settled down and to the left of the eye (where
    /// you'd sit), then wandering while it's curious, then drifting away.
    /// Always moving a little, so the mind keeps following it.
    pub fn cursor(&self, eye: Point, scale: f64) -> Point {
        let t = self.t() as f64;
        let s = 600.0 * scale;
        let (x, y) = match t {
            t if t < 3.2 => (-0.9, 0.55),
            t if t < 5.8 => (-0.4 + 0.9 * (t * 1.7).sin(), 0.3 + 0.6 * (t * 2.3).cos()),
            t if t < 10.2 => (-0.85, 0.5),
            t => (0.9 + 0.2 * (t - 10.2), -0.1),
        };
        // A slow drift on top, so it never sits perfectly still.
        let wobble = 0.03;
        Point {
            x: eye.x + s * (x + wobble * (t * 4.0).sin()),
            y: eye.y + s * (y + wobble * (t * 3.3).cos()),
        }
    }

    /// True once the performance is over.
    pub fn done(&self) -> bool {
        self.t() >= LENGTH
    }
}
