//! The `Mind` itself: the state machine that turns one frame of senses
//! into how the eye behaves (see the module docs in mod.rs for the modes).

use std::collections::HashMap;
use std::f64::consts::PI;

use super::faces::{CROWD_SIZE, face_in_room};
use super::{Face, Frame, Insight, Line, Mode, Say, Senses, Vec2, World, approach, clamp};
use crate::platform::desktop::Point;

/// A bang: louder than this, and this much louder than a moment ago.
pub const BANG_LEVEL: f64 = 0.62;
const BANG_JUMP: f64 = 0.3;

/// How long the eye takes to open at launch.
const WAKE_SECS: f64 = 0.9;

pub struct Mind {
    say: Say,
    rng: fastrand::Rng,
    t: f64,
    pub mode: Mode,
    gaze: Vec2,
    target: Vec2,
    gaze_rate: f64,
    pupil: f64,
    heat: f64,
    flare: f64,
    wide: f64,
    dim: f64,
    shake: f64,
    // `Option<T>` is Rust's "maybe a value": `Some(face)` or `None`.
    // It replaces JS's `null`, and the compiler makes us handle `None`.
    face: Option<Face>,
    last_face_at: f64,
    locked_since: f64,
    ever_seen: bool,
    said_gone: bool,
    crowd_index: usize,
    next_crowd_switch: f64,
    next_saccade: f64,
    baseline: f64,
    last_startle: f64,
    talk_time: f64,
    close_time: f64,
    last_cursor: Option<Point>,
    cursor_moved_at: f64,
    last_whisper: f64,
    line_cooldowns: HashMap<Line, f64>,
    was_blind: bool,
    warned_no_sight: bool,
    whispers: bool,
}

impl Mind {
    pub fn insight(&self) -> Insight {
        let mut cooldowns: Vec<(Line, f64)> =
            self.line_cooldowns.iter().map(|(&k, &v)| (k, v)).collect();
        cooldowns.sort_by(|a, b| a.1.total_cmp(&b.1));
        Insight {
            t: self.t,
            target: self.target,
            baseline: self.baseline,
            last_startle: self.last_startle,
            tracked: self.face,
            cooldowns,
            hush_until: self.last_whisper + 6.0,
        }
    }

    pub fn new(say: Say) -> Self {
        Self::with_rng(say, fastrand::Rng::new())
    }

    /// Same as `new`, with a chosen random source (tests seed it so every
    /// run makes the same "random" choices).
    pub fn with_rng(say: Say, rng: fastrand::Rng) -> Self {
        Mind {
            say,
            rng,
            t: 0.0,
            mode: Mode::Hunting,
            gaze: Vec2::default(),
            target: Vec2::default(),
            gaze_rate: 10.0,
            pupil: 0.5,
            heat: 0.0,
            flare: 0.0,
            wide: 0.05,
            dim: 0.0,
            shake: 0.0,
            face: None,
            last_face_at: f64::NEG_INFINITY,
            locked_since: 0.0,
            ever_seen: false,
            said_gone: false,
            crowd_index: 0,
            next_crowd_switch: 0.0,
            next_saccade: 0.0,
            baseline: 0.1,
            last_startle: f64::NEG_INFINITY,
            talk_time: 0.0,
            close_time: 0.0,
            last_cursor: None,
            cursor_moved_at: f64::NEG_INFINITY,
            last_whisper: f64::NEG_INFINITY,
            line_cooldowns: HashMap::new(),
            was_blind: false,
            warned_no_sight: false,
            whispers: true,
        }
    }

    /// `&mut self` borrows the mind mutably: only one piece of code may
    /// change it at a time, which the compiler checks for us.
    fn whisper(&mut self, kind: Line, cooldown: f64) {
        if !self.whispers {
            return;
        }
        if self.t - self.last_whisper < 6.0 {
            return;
        }
        // `.get()` returns an Option; `.copied().unwrap_or(0.0)` plays the
        // part of `?? 0` in JS.
        if self.t < self.line_cooldowns.get(&kind).copied().unwrap_or(0.0) {
            return;
        }
        self.last_whisper = self.t;
        self.line_cooldowns.insert(kind, self.t + cooldown);
        let texts = kind.texts();
        let text = texts[self.rng.usize(..texts.len())];
        (self.say)(text, kind);
    }

    /// Someone said "Hey, OG Blinki": a strong noticing, like a face that
    /// just came back. It flares, narrows its pupil, looks straight out at
    /// you (or keeps the face it already has) and answers. The answer
    /// skips the usual pause between whispers: being called deserves one.
    pub fn called(&mut self) {
        self.flare = self.flare.max(0.8);
        self.pupil = self.pupil.min(0.2);
        if self.face.is_none() {
            // Straight out of the screen is where you sit. Hold it a few
            // seconds before darting about again, and let the cursor go.
            self.target = Vec2::default();
            self.gaze_rate = 14.0;
            self.next_saccade = self.t + 3.0;
            self.cursor_moved_at = f64::NEG_INFINITY;
        }
        self.last_whisper = f64::NEG_INFINITY;
        self.whisper(Line::Called, 4.0);
    }

    /// Where your eyes are in the room, then the gaze that points the pupil
    /// from the eye's spot on the screen straight at them.
    ///
    /// The Electron app mapped the face's spot in the image onto the
    /// screen instead.
    /// With the eye in a top corner that aimed low, at your shoulder; this
    /// works out the real direction.
    fn gaze_at_face(face: &Face, world: Option<&World>) -> Vec2 {
        let Some(world) = world else {
            return Vec2::new(
                clamp((0.5 - face.x) * 2.2, -1.0, 1.0),
                clamp((0.5 - face.y) * 2.0, -1.0, 1.0),
            );
        };
        let [ux, uy, uz] = face_in_room(face);
        let (ux, uy) = (ux - world.look_offset.x, uy - world.look_offset.y);
        // The eye, in metres from the camera.
        let ex = (world.eye.x - world.camera.x) / world.px_per_m;
        let ey = (world.eye.y - world.camera.y) / world.px_per_m;
        // The shader turns the pupil toward normalize(g.x * 0.85, g.y * 0.7, 1),
        // so this is the gaze whose pupil points along (eye -> you).
        let (vx, vy) = ((ux - ex) / uz, (uy - ey) / uz);
        Vec2::new(clamp(vx / 0.85, -1.0, 1.0), clamp(-vy / 0.7, -1.0, 1.0))
    }

    /// Direction from `eye` to a point on screen, as if the point stood
    /// `depth` pixels in front of the glass. Screen y grows down; gaze y is up.
    pub fn gaze_at_point(sx: f64, sy: f64, eye: Point, depth: f64, gain: f64) -> Vec2 {
        let dx = sx - eye.x;
        let dy = sy - eye.y;
        let len = (dx * dx + dy * dy + depth * depth).sqrt();
        Vec2::new(
            clamp(dx / len * gain, -1.0, 1.0),
            clamp(-dy / len * gain, -1.0, 1.0),
        )
    }

    pub fn update(&mut self, dt: f64, senses: &Senses) -> Frame {
        self.t += dt;
        self.whispers = senses.whispers;
        let t = self.t;
        let world = senses.world.as_ref();
        let level = senses.level;

        // Cursor motion, for when there is no face to watch.
        if let Some(w) = world {
            let c = w.cursor;
            // A *let chain*: `if let ... && condition` (Rust 2024) checks both at once.
            if let Some(last) = self.last_cursor
                && (c.x - last.x).hypot(c.y - last.y) > 3.0 * w.scale
            {
                self.cursor_moved_at = t;
            }
            self.last_cursor = Some(c);
        }

        // Defaults each frame, refined by the mode below. Every branch
        // picks its own pupil, so it starts unset: Rust lets a variable be
        // assigned later as long as every path assigns it before it's read.
        let mut pupil_target: f64;
        let mut heat_target = 0.35;
        let mut wide_target = 1.0;
        let mut dim_target = 0.0;

        if senses.blind {
            self.mode = Mode::Blind;
            self.was_blind = true;
            dim_target = 1.0;
            pupil_target = 0.85;
            heat_target = 0.06;
            wide_target = 0.92;
            self.gaze_rate = 1.2;
            self.target = Vec2::new((t * 0.21).sin() * 0.5, (t * 0.13).sin() * 0.3);
        } else {
            if self.was_blind {
                self.was_blind = false;
                self.flare = 0.6;
                self.whisper(Line::Unblind, 5.0);
            }
            if !senses.can_see && !self.warned_no_sight && t > 4.0 {
                self.warned_no_sight = true;
                self.whisper(Line::NoSight, 600.0);
            }

            // An iterator chain: walk the faces, keep the confident ones,
            // and collect them into a new Vec (like `faces.filter(...)`).
            let visible: Vec<Face> = senses
                .faces
                .iter()
                .copied()
                .filter(|f| f.score > 0.55)
                .collect();
            if !visible.is_empty() {
                if self.mode != Mode::Locked {
                    let away = t - self.last_face_at;
                    if !self.ever_seen {
                        self.whisper(Line::First, 30.0);
                    } else if away > 12.0 {
                        self.whisper(Line::Returned, 25.0);
                    }
                    self.ever_seen = true;
                    self.said_gone = false;
                    self.locked_since = t;
                    self.flare = self.flare.max(if away > 12.0 { 0.7 } else { 0.25 });
                    self.mode = Mode::Locked;
                }
                self.last_face_at = t;

                // Keep watching the face it was watching: the visible one
                // nearest the last, so a stray detection can't steal the gaze.
                // (The Electron app always took the biggest.)
                let nearest = |p: Face| {
                    visible
                        .iter()
                        .copied()
                        .min_by(|a, b| {
                            let d = |f: &Face| (f.x - p.x).hypot(f.y - p.y);
                            d(a).total_cmp(&d(b))
                        })
                        .unwrap_or(visible[0])
                };
                let mut f = self.face.map_or(visible[0], nearest);

                // More than one face: look from one to the other. Only faces
                // near the biggest one's size count; a small steady "face"
                // (a photo, a hand) is not a second person.
                let crowd: Vec<Face> = visible
                    .iter()
                    .copied()
                    .filter(|c| c.size >= visible[0].size * CROWD_SIZE)
                    .collect();
                if crowd.len() > 1 {
                    self.whisper(Line::Crowd, 45.0);
                    if t > self.next_crowd_switch {
                        self.crowd_index = (self.crowd_index + 1) % crowd.len();
                        self.next_crowd_switch = t + 1.2 + self.rng.f64() * 1.4;
                    }
                    f = crowd[self.crowd_index.min(crowd.len() - 1)];
                    heat_target += 0.2;
                    self.gaze_rate = 16.0;
                } else {
                    self.gaze_rate = 9.0;
                }

                // Smooth the face we follow so detector noise doesn't jitter the gaze.
                let face = match self.face {
                    Some(prev) => Face {
                        x: approach(prev.x, f.x, 12.0, dt),
                        y: approach(prev.y, f.y, 12.0, dt),
                        size: approach(prev.size, f.size, 6.0, dt),
                        score: f.score,
                        aspect: f.aspect,
                    },
                    None => f,
                };
                self.face = Some(face);
                self.target = Self::gaze_at_face(&face, world);

                let close = clamp((face.size - 0.3) / 0.25, 0.0, 1.0);
                self.close_time = if close > 0.5 {
                    self.close_time + dt
                } else {
                    0.0
                };
                if self.close_time > 0.8 {
                    self.whisper(Line::Close, 25.0);
                }

                pupil_target = 0.14 - close * 0.12;
                heat_target += 0.12 + close * 0.4;
                wide_target = 1.0 + close * 0.12;

                if t - self.locked_since > 15.0 * 60.0 {
                    self.whisper(Line::Long, 15.0 * 60.0);
                }
            } else {
                let away = t - self.last_face_at;
                if self.mode == Mode::Locked && away < 1.5 {
                    // Detector flicker: hold the stare a moment before giving up.
                    pupil_target = 0.2;
                } else {
                    self.mode = Mode::Hunting;
                    self.face = None;
                    if self.ever_seen && away > 25.0 && !self.said_gone {
                        self.said_gone = true;
                        self.whisper(Line::Gone, 40.0);
                    }
                    let restless = clamp((away - 3.0) / 40.0, 0.0, 1.0);
                    pupil_target = 0.55 - restless * 0.15;
                    heat_target += restless * 0.3;

                    // A match guard (`if ...`) adds a condition to a pattern.
                    match world {
                        Some(w) if t - self.cursor_moved_at < 2.5 => {
                            self.target = Self::gaze_at_point(
                                w.cursor.x,
                                w.cursor.y,
                                w.eye,
                                260.0 * w.scale,
                                1.15,
                            );
                            self.gaze_rate = 14.0;
                            pupil_target = 0.35;
                        }
                        _ if t > self.next_saccade => {
                            // Dart somewhere new, faster the longer you've been gone.
                            let r = 0.4 + self.rng.f64() * 0.5;
                            let ang = self.rng.f64() * PI * 2.0;
                            self.target = Vec2::new(ang.cos() * r, ang.sin() * r * 0.8);
                            self.gaze_rate = 22.0;
                            self.next_saccade =
                                t + (0.6 + self.rng.f64() * 1.8) * (1.0 - restless * 0.6);
                        }
                        _ => {}
                    }
                }
            }

            // Hearing: the voice feeds the flames, a sudden bang makes it flinch.
            // The baseline catches up within a second, so steady talk never counts as a bang.
            let jump = level - self.baseline;
            self.baseline = approach(self.baseline, level, 1.5, dt);
            heat_target += level * 0.45;
            if level > BANG_LEVEL && jump > BANG_JUMP && t - self.last_startle > 1.5 {
                self.last_startle = t;
                self.flare = 1.0;
                self.shake = 1.0;
                self.pupil = 0.01;
                self.whisper(Line::Loud, 15.0);
            }
            self.talk_time = if level > 0.4 {
                self.talk_time + dt
            } else {
                (self.talk_time - dt * 0.5).max(0.0)
            };
            if self.talk_time > 3.0 {
                self.whisper(Line::Talking, 90.0);
            }
        }

        // Wake up: the fire catches and the eye opens over the first moment.
        // The Electron app took 2.5 s; 0.9 s matches the window's opening ease.
        let wake = clamp(t / WAKE_SECS, 0.0, 1.0);
        let ease = 1.0 - (1.0 - wake).powi(3);

        self.gaze.x = approach(self.gaze.x, self.target.x, self.gaze_rate, dt);
        self.gaze.y = approach(self.gaze.y, self.target.y, self.gaze_rate, dt);
        let pupil_rate = if self.mode == Mode::Locked { 5.0 } else { 3.0 };
        self.pupil = approach(self.pupil, pupil_target, pupil_rate, dt);
        self.heat = approach(self.heat, heat_target * ease, 3.0, dt);
        self.wide = approach(
            self.wide,
            (wide_target + self.flare * 0.3) * (0.05 + 0.95 * ease),
            6.0,
            dt,
        );
        self.dim = approach(self.dim, dim_target, 2.0, dt);
        self.flare = (self.flare - dt * 0.9).max(0.0);
        self.shake = (self.shake - dt * 2.5).max(0.0);

        // Eyes are never perfectly still.
        // flare (0..1) spikes on a startle or a return and decays within about a second;
        // heat is the steady burn, fed by closeness, crowds, restlessness, and the voice.
        let tremor = 0.012;
        let jitter = self.shake * 0.012;
        Frame {
            gaze: Vec2::new(
                self.gaze.x + (t * 13.1).sin() * tremor * (t * 2.3).sin(),
                self.gaze.y + (t * 11.7).cos() * tremor * (t * 1.9).sin(),
            ),
            pupil: self.pupil,
            heat: self.heat,
            flare: self.flare,
            wide: self.wide,
            dim: self.dim,
            shake: Vec2::new(
                (self.rng.f64() - 0.5) * jitter,
                (self.rng.f64() - 0.5) * jitter,
            ),
            mode: self.mode,
        }
    }
}

// Unit tests live next to the code. `#[cfg(test)]` compiles this module
// only for `cargo test`, so none of it ends up in the app.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::desktop::Rect;
    use std::cell::RefCell;
    use std::rc::Rc;

    const DT: f64 = 1.0 / 60.0;

    /// A mind whose whispers land in a shared list we can inspect.
    /// `Rc<RefCell<..>>` is shared ownership (Rc) of a value we can still
    /// mutate (RefCell): the closure holds one handle, the test the other.
    /// Every whisper so far, shared with the test. A `type` alias names a
    /// long type once so the signatures below stay readable.
    type Said = Rc<RefCell<Vec<(&'static str, Line)>>>;

    fn mind() -> (Mind, Said) {
        let said = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&said);
        // `move` makes the closure take ownership of `sink`.
        let say: Say = Box::new(move |text, kind| sink.borrow_mut().push((text, kind)));
        (Mind::with_rng(say, fastrand::Rng::with_seed(7)), said)
    }

    fn senses(faces: &[Face]) -> Senses<'_> {
        Senses {
            faces,
            can_see: true,
            level: 0.0,
            world: None,
            blind: false,
            whispers: true,
        }
    }

    fn face(x: f64, y: f64) -> Face {
        Face {
            x,
            y,
            size: 0.2,
            score: 0.9,
            aspect: 16.0 / 9.0,
        }
    }

    /// Runs `seconds` of frames with the same senses; returns the last frame.
    fn run(m: &mut Mind, seconds: f64, s: &Senses) -> Frame {
        let mut last = m.update(DT, s);
        for _ in 1..(seconds / DT).round() as usize {
            last = m.update(DT, s);
        }
        last
    }

    fn kinds(said: &Said) -> Vec<Line> {
        said.borrow().iter().map(|&(_, k)| k).collect()
    }

    fn world(cursor: Point) -> World {
        World {
            cursor,
            eye: Point {
                x: 1000.0,
                y: 200.0,
            },
            display: Rect {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            },
            scale: 1.0,
            camera: Point { x: 960.0, y: 0.0 }, // top middle
            px_per_m: 4000.0,                   // a 1920 px wide, 48 cm wide screen
            look_offset: Vec2::default(),
        }
    }

    #[test]
    fn starts_hunting_and_wakes_up() {
        let (mut m, said) = mind();
        let first = m.update(DT, &senses(&[]));
        assert_eq!(first.mode, Mode::Hunting);
        assert!(first.wide < 0.1, "starts nearly shut, got {}", first.wide);
        let later = run(&mut m, 3.0, &senses(&[]));
        assert!(
            later.wide > 0.9,
            "open after the wake-up, got {}",
            later.wide
        );
        assert!(said.borrow().is_empty());
    }

    #[test]
    fn locks_on_a_face_and_greets_it() {
        let (mut m, said) = mind();
        let f = run(&mut m, 2.0, &senses(&[face(0.5, 0.5)]));
        assert_eq!(f.mode, Mode::Locked);
        assert!(f.pupil < 0.2, "pupil narrows to a slit, got {}", f.pupil);
        assert_eq!(said.borrow()[0], ("I see you.", Line::First));
    }

    #[test]
    fn ignores_unsure_faces() {
        let (mut m, _) = mind();
        let unsure = Face {
            score: 0.5,
            ..face(0.5, 0.5)
        };
        assert_eq!(run(&mut m, 1.0, &senses(&[unsure])).mode, Mode::Hunting);
    }

    /// The eye in the top right corner of a 1920x1080 screen, you centred
    /// in front of it. The answer should point left (toward the middle) and
    /// down (toward your eyes, below the top edge).
    #[test]
    fn from_the_corner_it_looks_down_and_in_at_you() {
        let (mut m, _) = mind();
        let mut w = world(Point { x: 0.0, y: 0.0 });
        w.eye = Point {
            x: 1760.0,
            y: 160.0,
        };
        // Eyes a little below the frame's middle (you sit lower than the
        // camera on top of the screen), a face 18% of the frame wide (~60 cm away).
        let you = Face {
            size: 0.18,
            ..face(0.5, 0.65)
        };
        let f = run(
            &mut m,
            2.0,
            &Senses {
                world: Some(w),
                ..senses(&[you])
            },
        );
        assert!(f.gaze.x < -0.3, "looks in toward you: {:?}", f.gaze);
        assert!(f.gaze.y < -0.03, "and down: {:?}", f.gaze);
        assert!(f.gaze.y > -0.5, "but not at your chest: {:?}", f.gaze);
    }

    #[test]
    fn a_small_stray_face_does_not_steal_the_gaze() {
        let (mut m, said) = mind();
        let you = Face {
            size: 0.13,
            ..face(0.5, 0.6)
        };
        let stray = Face {
            size: 0.09,
            ..face(0.7, 0.9)
        };
        let f = run(&mut m, 4.0, &senses(&[you, stray]));
        let alone = run(&mut mind().0, 4.0, &senses(&[you]));
        assert!(
            (f.gaze.x - alone.gaze.x).abs() < 0.05,
            "{:?} vs {:?}",
            f.gaze,
            alone.gaze
        );
        assert!(!kinds(&said).contains(&Line::Crowd));
    }

    #[test]
    fn two_people_still_make_a_crowd() {
        let (mut m, said) = mind();
        let a = Face {
            size: 0.13,
            ..face(0.3, 0.6)
        };
        let b = Face {
            size: 0.12,
            ..face(0.7, 0.6)
        };
        run(&mut m, 8.0, &senses(&[a, b]));
        assert!(kinds(&said).contains(&Line::Crowd));
    }

    /// Same face, same eye; only the camera moves. With the camera at the
    /// bottom of the screen, a face in the middle of its picture is down
    /// there, so the eye in the top corner looks further down.
    #[test]
    fn where_the_camera_sits_changes_where_it_looks() {
        let look = |camera: Point| {
            let mut w = world(Point { x: 0.0, y: 0.0 });
            w.eye = Point {
                x: 1760.0,
                y: 160.0,
            };
            w.camera = camera;
            let (mut m, _) = mind();
            let f = Face {
                size: 0.18,
                ..face(0.5, 0.5)
            };
            run(
                &mut m,
                2.0,
                &Senses {
                    world: Some(w),
                    ..senses(&[f])
                },
            )
            .gaze
        };
        let top = look(Point { x: 960.0, y: 0.0 });
        let bottom = look(Point {
            x: 960.0,
            y: 1080.0,
        });
        let right = look(Point {
            x: 1920.0,
            y: 540.0,
        });
        assert!(bottom.y < top.y - 0.3, "top {top:?} bottom {bottom:?}");
        assert!(right.x > top.x + 0.3, "top {top:?} right {right:?}");
    }

    #[test]
    fn leaning_right_turns_it_right() {
        // You lean to your right: in the unmirrored camera image you move left.
        let mut w = world(Point { x: 0.0, y: 0.0 });
        w.eye = Point {
            x: 1760.0,
            y: 160.0,
        };
        let gaze = |x| {
            let (mut m, _) = mind();
            let f = Face {
                size: 0.18,
                ..face(x, 0.5)
            };
            run(
                &mut m,
                2.0,
                &Senses {
                    world: Some(w),
                    ..senses(&[f])
                },
            )
            .gaze
        };
        assert!(gaze(0.3).x > gaze(0.5).x + 0.2);
    }

    #[test]
    fn looks_toward_the_face() {
        // Camera frames are mirrored: a face at small x is on the viewer's
        // right, so the eye looks right (+x). Small y is high: look up.
        let (mut m, _) = mind();
        let f = run(&mut m, 2.0, &senses(&[face(0.2, 0.2)]));
        assert!(f.gaze.x > 0.5 && f.gaze.y > 0.4, "gaze {:?}", f.gaze);
    }

    #[test]
    fn holds_the_stare_through_flicker_then_hunts() {
        let (mut m, _) = mind();
        run(&mut m, 1.0, &senses(&[face(0.5, 0.5)]));
        assert_eq!(run(&mut m, 1.0, &senses(&[])).mode, Mode::Locked);
        assert_eq!(run(&mut m, 1.0, &senses(&[])).mode, Mode::Hunting);
    }

    #[test]
    fn says_where_are_you_once_after_a_long_absence() {
        let (mut m, said) = mind();
        run(&mut m, 1.0, &senses(&[face(0.5, 0.5)]));
        run(&mut m, 60.0, &senses(&[]));
        let gone = kinds(&said).iter().filter(|&&k| k == Line::Gone).count();
        assert_eq!(gone, 1);
    }

    #[test]
    fn welcomes_you_back() {
        let (mut m, said) = mind();
        run(&mut m, 1.0, &senses(&[face(0.5, 0.5)]));
        run(&mut m, 20.0, &senses(&[]));
        run(&mut m, 0.5, &senses(&[face(0.5, 0.5)]));
        assert_eq!(kinds(&said).last(), Some(&Line::Returned));
    }

    #[test]
    fn going_blind_dims_and_waking_flares() {
        let (mut m, said) = mind();
        let blind = Senses {
            blind: true,
            ..senses(&[])
        };
        let f = run(&mut m, 4.0, &blind);
        assert_eq!(f.mode, Mode::Blind);
        assert!(f.dim > 0.99, "dim {}", f.dim);
        let f = m.update(DT, &senses(&[]));
        assert!(f.flare > 0.5, "flare {}", f.flare);
        assert_eq!(kinds(&said), [Line::Unblind]);
    }

    #[test]
    fn a_bang_makes_it_flinch() {
        let (mut m, said) = mind();
        run(&mut m, 3.0, &senses(&[]));
        let bang = Senses {
            level: 0.9,
            ..senses(&[])
        };
        let f = m.update(DT, &bang);
        assert!(f.flare > 0.95, "flare {}", f.flare);
        assert!(f.pupil < 0.05, "pupil {}", f.pupil);
        assert_eq!(kinds(&said), [Line::Loud]);
    }

    #[test]
    fn follows_a_moving_cursor() {
        let (mut m, _) = mind();
        run(&mut m, 3.0, &senses(&[]));
        // Sweep the cursor across the bottom left of the screen.
        let mut f = None;
        for i in 0..60 {
            let p = Point {
                x: 300.0 - i as f64 * 4.0,
                y: 900.0,
            };
            f = Some(m.update(
                DT,
                &Senses {
                    world: Some(world(p)),
                    ..senses(&[])
                },
            ));
        }
        let f = f.unwrap();
        assert_eq!(f.mode, Mode::Hunting);
        assert!(f.gaze.x < -0.5 && f.gaze.y < -0.3, "gaze {:?}", f.gaze);
    }

    #[test]
    fn whispers_wait_their_turn() {
        // Two lines due at once: the second waits out the six second hush.
        let (mut m, said) = mind();
        run(&mut m, 3.0, &senses(&[]));
        // The array is bound to a name so it outlives the borrow `senses` takes.
        let faces = [face(0.5, 0.5)];
        let bang = Senses {
            level: 0.9,
            ..senses(&faces)
        };
        m.update(DT, &bang);
        assert_eq!(kinds(&said), [Line::First]);
    }

    #[test]
    fn says_nothing_when_whispers_are_off() {
        let (mut m, said) = mind();
        let faces = [face(0.5, 0.5)];
        let quiet = Senses {
            whispers: false,
            ..senses(&faces)
        };
        run(&mut m, 2.0, &quiet);
        assert!(said.borrow().is_empty());
    }

    #[test]
    fn answers_when_called_even_right_after_a_whisper() {
        let (mut m, said) = mind();
        run(&mut m, 1.0, &senses(&[face(0.5, 0.5)])); // "I see you."
        m.called();
        assert_eq!(kinds(&said), [Line::First, Line::Called]);
        let f = m.update(DT, &senses(&[face(0.5, 0.5)]));
        assert!(f.flare > 0.7, "it flares, got {}", f.flare);
        m.called();
        assert_eq!(said.borrow().len(), 2, "not twice in a row");
    }

    #[test]
    fn warns_once_when_it_cannot_see() {
        let (mut m, said) = mind();
        let no_camera = Senses {
            can_see: false,
            ..senses(&[])
        };
        run(&mut m, 30.0, &no_camera);
        assert_eq!(kinds(&said), [Line::NoSight]);
    }

    #[test]
    fn gaze_at_point_is_clamped_and_points_the_right_way() {
        let eye = Point { x: 0.0, y: 0.0 };
        let g = Mind::gaze_at_point(10_000.0, -10_000.0, eye, 1.0, 5.0);
        assert_eq!(g, Vec2::new(1.0, 1.0));
        let g = Mind::gaze_at_point(0.0, 0.0, eye, 100.0, 1.0);
        assert_eq!(g, Vec2::new(0.0, 0.0));
    }

    fn count(said: &Said, kind: Line) -> usize {
        kinds(said).iter().filter(|&&k| k == kind).count()
    }

    #[test]
    fn a_line_waits_out_its_own_cooldown() {
        // Loud has a 15 s cooldown, longer than the 6 s hush between any two
        // whispers. A second bang at 10 s still flinches but stays silent;
        // a third, after the cooldown, speaks again.
        let (mut m, said) = mind();
        let quiet = senses(&[]);
        let bang = Senses {
            level: 0.9,
            ..senses(&[])
        };
        run(&mut m, 3.0, &quiet);
        m.update(DT, &bang);
        run(&mut m, 7.0, &quiet);
        let f = m.update(DT, &bang);
        assert!(f.flare > 0.95, "it still flinches, flare {}", f.flare);
        assert_eq!(count(&said, Line::Loud), 1, "but says nothing yet");
        run(&mut m, 10.0, &quiet);
        m.update(DT, &bang);
        assert_eq!(count(&said, Line::Loud), 2);
    }

    #[test]
    fn steady_talk_is_heard_but_is_not_a_bang() {
        let (mut m, said) = mind();
        run(&mut m, 3.0, &senses(&[]));
        let talk = Senses {
            level: 0.5,
            ..senses(&[])
        };
        let f = run(&mut m, 4.0, &talk);
        assert!(f.flare < 0.05, "no flinch, flare {}", f.flare);
        assert_eq!(kinds(&said), [Line::Talking]);
    }

    #[test]
    fn a_short_absence_is_not_a_return() {
        // Gone for 5 s (past the flicker hold, under the 12 s "returned"
        // threshold): it hunts, then locks again without a welcome.
        let (mut m, said) = mind();
        let you = [face(0.5, 0.5)];
        run(&mut m, 1.0, &senses(&you));
        assert_eq!(run(&mut m, 5.0, &senses(&[])).mode, Mode::Hunting);
        assert_eq!(run(&mut m, 0.5, &senses(&you)).mode, Mode::Locked);
        assert_eq!(kinds(&said), [Line::First]);
    }

    #[test]
    fn blind_wins_over_a_face_in_view() {
        let (mut m, said) = mind();
        let faces = [face(0.5, 0.5)];
        let blind = Senses {
            blind: true,
            ..senses(&faces)
        };
        assert_eq!(run(&mut m, 2.0, &blind).mode, Mode::Blind);
        assert!(said.borrow().is_empty(), "it saw no one");
    }

    #[test]
    fn says_still_watching_after_fifteen_minutes() {
        let (mut m, said) = mind();
        let you = [face(0.5, 0.5)];
        run(&mut m, 14.0 * 60.0, &senses(&you));
        assert_eq!(count(&said, Line::Long), 0);
        run(&mut m, 2.0 * 60.0, &senses(&you));
        assert_eq!(count(&said, Line::Long), 1);
    }

    #[test]
    fn the_same_seed_makes_the_same_eye() {
        // Hunting darts are random; with a seeded Rng two minds agree exactly.
        let (mut a, _) = mind();
        let (mut b, _) = mind();
        let fa = run(&mut a, 10.0, &senses(&[]));
        let fb = run(&mut b, 10.0, &senses(&[]));
        assert_eq!((fa.gaze, fa.pupil, fa.shake), (fb.gaze, fb.pupil, fb.shake));
    }
}
