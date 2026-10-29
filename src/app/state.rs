//! Small pieces of the `App`'s state, grouped by job. Each is a struct
//! with the methods that only touch its own fields: Rust has no classes,
//! but a struct plus an `impl` block does the same work.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::mind::{self, Line};
use crate::ui::senses_panel::{LEVEL_HISTORY, Snapshot};

/// How often the terminal heartbeat line is printed.
const BEAT_EVERY: Duration = Duration::from_secs(3);
/// Hours after midnight before this count as the small hours.
const LATE_UNTIL_HOUR: u32 = 5;
/// Away this long (no face, no input) and the late night starts over.
const AWAY_RESETS_LATE: Duration = Duration::from_secs(300);

/// When frames were drawn: the time step between them, when the next one is
/// due, and the frame rate for the heartbeat line.
pub(super) struct FrameClock {
    last_frame: Instant,
    /// When the next frame is due (see pacing.rs).
    pub(super) next_frame: Instant,
    /// Frames drawn since the last heartbeat line, to print the frame rate.
    frames: u32,
    last_beat: Instant,
}

impl FrameClock {
    pub(super) fn new(now: Instant) -> Self {
        FrameClock {
            last_frame: now,
            next_frame: now,
            frames: 0,
            last_beat: now,
        }
    }

    /// Seconds since the last frame (at most 0.1, so a stall doesn't jump),
    /// and marks `now` as this frame's time.
    pub(super) fn tick(&mut self, now: Instant) -> f64 {
        let dt = (now - self.last_frame).as_secs_f64().min(0.1);
        self.last_frame = now;
        dt
    }

    /// Counts a frame. Every few seconds returns the frame rate since the
    /// last time, which is when the heartbeat line is due.
    pub(super) fn beat(&mut self, now: Instant) -> Option<f64> {
        self.frames += 1;
        if self.last_beat.elapsed() < BEAT_EVERY {
            return None;
        }
        let fps = self.frames as f64 / self.last_beat.elapsed().as_secs_f64();
        self.frames = 0;
        self.last_beat = now;
        Some(fps)
    }
}

/// Every whisper so far: when, what kind, and the words.
type Log = Rc<RefCell<Vec<(Instant, Line, &'static str)>>>;

/// The whispers' bookkeeping: what the mind has said, what has been spoken
/// aloud, and what waits to go on screen.
pub(super) struct WhisperBook {
    /// The latest whisper and when, shared with the mind's `say` closure.
    /// `Rc<Cell<..>>`: both own it (Rc), and a Copy value inside a Cell can
    /// be swapped even through a shared handle.
    last_said: Rc<Cell<Option<(Line, Instant)>>>,
    /// Every whisper so far, for the Mind lens. A `Vec` isn't Copy, so it
    /// sits in a `RefCell`, which lends it out one `borrow_mut` at a time.
    log: Log,
    /// How many logged whispers the voice has seen.
    spoken: usize,
    /// A whisper to put on screen once the frame's GPU work is at hand.
    pub(super) pending: Option<String>,
}

impl WhisperBook {
    pub(super) fn new() -> Self {
        WhisperBook {
            last_said: Rc::new(Cell::new(None)),
            log: Rc::new(RefCell::new(Vec::new())),
            spoken: 0,
            pending: None,
        }
    }

    /// The mind's `say`: for now its whispers go to the terminal; this also
    /// notes the latest one, because some moods follow what was just said,
    /// and logs it. `Rc::clone` hands the closure its own handles.
    pub(super) fn say(&self) -> mind::Say {
        let heard = Rc::clone(&self.last_said);
        let log = Rc::clone(&self.log);
        Box::new(move |text, kind| {
            eprintln!("whisper ({kind:?}): {text}");
            heard.set(Some((kind, Instant::now())));
            log.borrow_mut().push((Instant::now(), kind, text));
        })
    }

    /// Logs a whisper the mind didn't say (the --demo script's), so Blinki's
    /// Senses lists it; it goes on screen and is spoken like the mind's.
    pub(super) fn log(&self, kind: Line, text: &'static str) {
        self.log.borrow_mut().push((Instant::now(), kind, text));
    }

    /// The newest logged whisper, if any came since the last call.
    pub(super) fn fresh(&mut self) -> Option<&'static str> {
        let log = self.log.borrow();
        let fresh = log.len() > self.spoken;
        self.spoken = log.len();
        fresh
            .then(|| log.last().map(|(_, _, text)| *text))
            .flatten()
    }

    /// The latest whisper's kind, and how many seconds ago it was said.
    pub(super) fn last_said(&self) -> Option<(Line, f32)> {
        self.last_said
            .get()
            .map(|(kind, at)| (kind, at.elapsed().as_secs_f32()))
    }

    /// Every whisper, stamped on the mind's clock (`now` is its time now).
    pub(super) fn timeline(&self, now: f64) -> Vec<(f64, Line, &'static str)> {
        self.log
            .borrow()
            .iter()
            .map(|(at, kind, text)| (now - at.elapsed().as_secs_f64(), *kind, *text))
            .collect()
    }
}

/// What Blinki's Senses has gathered: the latest snapshot, and the sound
/// level and flinches over the last few seconds, for its graph.
pub(super) struct SensesHistory {
    /// What the panel shows this frame, while it's open.
    pub(super) snapshot: Option<Snapshot>,
    levels: VecDeque<f32>,
    /// One flag per entry in `levels`: did it flinch on that frame?
    flinch_marks: VecDeque<bool>,
    last_startle: f64,
}

impl SensesHistory {
    pub(super) fn new() -> Self {
        SensesHistory {
            snapshot: None,
            levels: VecDeque::new(),
            flinch_marks: VecDeque::new(),
            last_startle: f64::NEG_INFINITY,
        }
    }

    /// Adds this frame's sound level, and whether it flinched: a new startle
    /// time means it flinched this frame. Keeps the last `LEVEL_HISTORY`.
    pub(super) fn record(&mut self, level: f64, last_startle: f64) {
        let flinched = last_startle > self.last_startle;
        self.last_startle = last_startle;
        self.levels.push_back(level as f32);
        self.flinch_marks.push_back(flinched);
        while self.levels.len() > LEVEL_HISTORY {
            self.levels.pop_front();
            self.flinch_marks.pop_front();
        }
    }

    /// The recent sound levels, oldest first.
    pub(super) fn levels(&self) -> VecDeque<f32> {
        self.levels.clone()
    }

    /// Where in `levels` it flinched.
    pub(super) fn flinches(&self) -> Vec<usize> {
        self.flinch_marks
            .iter()
            .enumerate()
            .filter(|(_, f)| **f)
            .map(|(i, _)| i)
            .collect()
    }
}

/// The late-night worry: how long you've been up past midnight, and when
/// it last told you to rest, for the Worried mood.
pub(super) struct LateNight {
    /// Seconds you've been up and here past midnight tonight.
    up_late: f32,
    /// When you were last here (face or keyboard/mouse).
    last_here: Instant,
    /// When it last told you to rest, so it doesn't nag.
    last_worry: Option<Instant>,
}

impl LateNight {
    pub(super) fn new(now: Instant) -> Self {
        LateNight {
            up_late: 0.0,
            last_here: now,
            last_worry: None,
        }
    }

    /// Up late: time here (face, or input in the last 30 s) after
    /// midnight. Five minutes away, or morning, starts it over.
    pub(super) fn track(&mut self, now: Instant, here: bool, hour: u32, dt: f32) {
        if here {
            self.last_here = now;
        }
        let small_hours = hour < LATE_UNTIL_HOUR;
        if !small_hours || self.last_here.elapsed() > AWAY_RESETS_LATE {
            self.up_late = 0.0;
        } else if here {
            self.up_late += dt;
        }
    }

    /// Seconds up and here past midnight tonight.
    pub(super) fn secs(&self) -> f32 {
        self.up_late
    }

    /// Whether it's been `every_secs` since it last told you to rest (or it
    /// never has). If so, it counts as telling you now.
    pub(super) fn time_to_worry(&mut self, now: Instant, every_secs: f32) -> bool {
        // `is_none_or`: true for `None`, else whatever the test says.
        let due = self
            .last_worry
            .is_none_or(|t| t.elapsed().as_secs_f32() > every_secs);
        if due {
            self.last_worry = Some(now);
        }
        due
    }
}

/// When it last saw a face and heard anything louder than quiet, for the
/// moods and for slowing down when nobody's around.
pub(super) struct Company {
    /// When the app started: "no face yet" counts from here.
    since: Instant,
    last_face: Option<Instant>,
    last_noise: Instant,
}

impl Company {
    pub(super) fn new(now: Instant) -> Self {
        Company {
            since: now,
            last_face: None,
            last_noise: now,
        }
    }

    /// Notes this frame: whether there was a face, and how loud it was.
    pub(super) fn notice(&mut self, now: Instant, face: bool, level: f64) {
        if face {
            self.last_face = Some(now);
        }
        if level > 0.3 {
            self.last_noise = now;
        }
    }

    /// How long since it last saw a face (or since it started, if never).
    pub(super) fn no_face_for(&self) -> Duration {
        self.last_face.map_or(self.since.elapsed(), |t| t.elapsed())
    }

    /// How long since it last heard anything louder than quiet.
    pub(super) fn quiet_for(&self) -> Duration {
        self.last_noise.elapsed()
    }
}
