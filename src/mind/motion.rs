//! Motion: the body that carries out what the mind decides.
//!
//! The mind (`Mind`) says where to look, how wide the pupil is, how hot the
//! fire burns. Those numbers are right, but on their own they move like a
//! machine: every change is the same smooth exponential slide. Motion sits
//! between the mind and the shader and gives the eye a body:
//!
//!   gaze    a spring with weight: it swings a little past a fast turn and
//!           settles, and it is never perfectly still (slow drift plus tiny
//!           jumps, the way real eyes hold a point)
//!   pupil   eased, with a slow restless wobble (hippus)
//!   breath  a slow, uneven in-and-out that quickens when it's worked up
//!   wince   a flinch that snaps in on a startle and fades out
//!   roll    the sphere turning a little about the view axis, very slowly
//!   morph   0 at rest (a perfect sphere); only a flinch bends it, briefly
//!
//! Everything is driven by `dt`, the seconds since the last frame, so it
//! looks the same at 30 or 240 frames per second, and it stays stable at the
//! 0.1 s frames the app clamps a hiccup to.

use std::f64::consts::{PI, TAU};

use super::approach;
use crate::mind::{Frame, Mode};

/// A spring's character: how fast it is (`f`, in hertz), how quickly its
/// swinging dies out (`zeta`: below 1 it overshoots, 1 just settles), and how
/// it answers a sudden change (`r`: 0 eases in, 1 answers at once, above 1
/// leaps ahead and swings past).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Feel {
    pub f: f64,
    pub zeta: f64,
    pub r: f64,
}

impl Feel {
    /// Locked on a face: quick and sure, a small swing past and settled.
    pub const LOCKED: Feel = Feel {
        f: 3.2,
        zeta: 0.75,
        r: 1.1,
    };
    /// Hunting: lazier and looser, so each dart swings and settles.
    pub const HUNTING: Feel = Feel {
        f: 2.4,
        zeta: 0.62,
        r: 1.25,
    };
    /// Blind: heavy and dreamy, no swing at all.
    pub const BLIND: Feel = Feel {
        f: 1.3,
        zeta: 0.95,
        r: 0.5,
    };

    fn for_mode(mode: Mode) -> Feel {
        // A `match` on an enum must name every choice; add a Mode and this
        // stops compiling until it's given a feel.
        match mode {
            Mode::Locked => Feel::LOCKED,
            Mode::Hunting => Feel::HUNTING,
            Mode::Blind => Feel::BLIND,
        }
    }
}

/// A "second-order system": a spring and damper that chases a target.
///
/// It solves `k2·y'' + k1·y' + y = x + k3·x'`, where `x` is where it's told
/// to be and `y` is where it is. The three `k`s come from a `Feel`. This is
/// the well-known recipe for procedural animation (t3ssel8r's "Giving
/// Personality to Procedural Animations").
///
/// Each short step is *semi-implicit Euler*: move by the current
/// velocity, then update the velocity. A plain spring blows up when the step
/// is long compared to how stiff the spring is, so `k2` is raised to the
/// smallest value that is still stable for this `dt`. A long frame then
/// costs a little sharpness instead of exploding.
#[derive(Clone, Copy, Debug)]
struct Spring {
    /// Where it is, and how fast it's moving (per second).
    y: f64,
    v: f64,
    /// Last frame's target, to tell how fast the target moves.
    x_prev: f64,
}

impl Spring {
    fn new(at: f64) -> Self {
        Spring {
            y: at,
            v: 0.0,
            x_prev: at,
        }
    }

    /// Advances `dt` seconds toward `x`. A long frame is cut into short
    /// steps, with the target sliding from last frame's spot to this one's
    /// across them: one 0.1 s step would read a jump as an enormous target
    /// speed and fling the eye far past it.
    fn step(&mut self, x: f64, feel: Feel, dt: f64) -> f64 {
        let n = (dt / SPRING_STEP).ceil().max(1.0);
        let (from, h) = (self.x_prev, dt / n);
        for i in 1..=n as usize {
            self.substep(from + (x - from) * (i as f64 / n), feel, h);
        }
        self.y
    }

    fn substep(&mut self, x: f64, feel: Feel, dt: f64) {
        let w = TAU * feel.f;
        let k1 = feel.zeta / (PI * feel.f);
        let k2 = 1.0 / (w * w);
        let k3 = feel.r * feel.zeta / w;
        // How fast the target itself is moving.
        let xv = (x - self.x_prev) / dt;
        self.x_prev = x;
        // The stability clamp: the smallest k2 this dt can integrate safely.
        let k2 = k2.max(dt * dt / 2.0 + dt * k1 / 2.0).max(dt * k1);
        self.y += dt * self.v;
        self.v += dt * (x + k3 * xv - self.y - k1 * self.v) / k2;
    }
}

/// The longest single step a spring takes, in seconds.
const SPRING_STEP: f64 = 1.0 / 120.0;

/// Smooth random wandering: value noise in time.
///
/// Every `1 / rate` seconds it picks a new random number in -1..1, and in
/// between it glides from the last one to the next with an S-curve, so it
/// has no corners. Several of these at different rates, the slow ones
/// louder, add up to "1/f" wandering: lazy drift with a little fine grain,
/// the kind of motion living things have and sine waves don't.
#[derive(Clone, Copy, Debug)]
struct Wander {
    from: f64,
    to: f64,
    /// 0..1, how far along the glide from `from` to `to`.
    phase: f64,
    /// New values per second.
    rate: f64,
}

impl Wander {
    fn new(rate: f64, rng: &mut fastrand::Rng) -> Self {
        Wander {
            from: rng.f64() * 2.0 - 1.0,
            to: rng.f64() * 2.0 - 1.0,
            phase: rng.f64(),
            rate,
        }
    }

    fn step(&mut self, dt: f64, rng: &mut fastrand::Rng) -> f64 {
        self.phase += dt * self.rate;
        // `while`, not `if`: one long frame may pass more than one value.
        while self.phase >= 1.0 {
            self.phase -= 1.0;
            self.from = self.to;
            self.to = rng.f64() * 2.0 - 1.0;
        }
        let t = self.phase;
        let s = t * t * (3.0 - 2.0 * t);
        self.from + (self.to - self.from) * s
    }
}

/// Three `Wander`s an octave-ish apart, each half as loud as the last.
/// The result stays in -1..1.
#[derive(Clone, Copy, Debug)]
struct Drift([Wander; 3]);

impl Drift {
    fn new(rate: f64, rng: &mut fastrand::Rng) -> Self {
        Drift([
            Wander::new(rate, rng),
            Wander::new(rate * 2.3, rng),
            Wander::new(rate * 5.1, rng),
        ])
    }

    fn step(&mut self, dt: f64, rng: &mut fastrand::Rng) -> f64 {
        let [a, b, c] = &mut self.0;
        (a.step(dt, rng) + 0.5 * b.step(dt, rng) + 0.25 * c.step(dt, rng)) / 1.75
    }
}

/// How far the eye drifts while holding a point, in gaze units, each way.
const DRIFT: f64 = 0.005;
/// How far a microsaccade may land from the point it holds.
const MICRO: f64 = 0.007;
/// The pupil's restless wobble, as a fraction of its size.
const HIPPUS: f64 = 0.035;
/// The resting breath: seconds per breath.
const BREATH_SECS: f64 = 5.5;
/// How much each breath brightens and dims the fire, as a fraction of heat.
const BREATH_HEAT: f64 = 0.04;
/// How far the sphere wanders round its view axis, in radians.
const ROLL: f64 = 0.14;
/// How much a flinch may bend the sphere (morph = MORPH_WINCE × wince).
const MORPH_WINCE: f64 = 0.6;
/// A jump in the mind's flare this big in one frame is a startle.
const STARTLE_JUMP: f64 = 0.15;

/// What Motion hands back each frame: the mind's frame, brought to life.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Moved {
    pub gaze: [f32; 2],
    /// Gaze units per second.
    pub gaze_vel: [f32; 2],
    pub pupil: f32,
    /// Per second; above 0 while it opens.
    pub pupil_vel: f32,
    pub heat: f32,
    /// -1..1, inhale above 0.
    pub breath: f32,
    /// 0..1.
    pub wince: f32,
    /// 0 at rest; rises only with a wince.
    pub morph: f32,
    /// Radians.
    pub roll: f32,
    /// 0..1, how hard the shader may work. Always 1 for now.
    pub quality: f32,
}

/// The body. One per app; `update` it once a frame with the mind's frame.
pub struct Motion {
    rng: fastrand::Rng,
    /// False until the first frame, which places everything at once
    /// instead of springing in from zero.
    started: bool,
    feel: Feel,
    gaze: [Spring; 2],
    drift: [Drift; 2],
    /// Where the latest microsaccade landed, relative to the point it holds.
    micro: [f64; 2],
    next_micro: f64,
    pupil: f64,
    hippus: Drift,
    breath_phase: f64,
    breath_depth: f64,
    /// Wobbles in the breath's pace and depth, so no two are alike.
    breath_pace: Wander,
    breath_size: Wander,
    roll: Drift,
    flare_prev: f64,
    /// The flinch it is heading toward, and the flinch it shows.
    kick: f64,
    wince: f64,
    last: Moved,
}

impl Motion {
    pub fn new() -> Self {
        Self::with_rng(fastrand::Rng::new())
    }

    /// Same as `new`, with a chosen random source: a seeded one makes the
    /// same "random" motion every run, which the tests rely on.
    pub fn with_rng(mut rng: fastrand::Rng) -> Self {
        // Each field below borrows `rng` mutably in turn. Rust allows that
        // because each borrow ends before the next begins.
        let drift = [Drift::new(0.35, &mut rng), Drift::new(0.35, &mut rng)];
        let hippus = Drift::new(0.22, &mut rng);
        let breath_pace = Wander::new(0.08, &mut rng);
        let breath_size = Wander::new(0.11, &mut rng);
        let roll = Drift::new(0.035, &mut rng);
        let next_micro = 0.4 + rng.f64();
        Motion {
            rng,
            started: false,
            feel: Feel::HUNTING,
            gaze: [Spring::new(0.0), Spring::new(0.0)],
            drift,
            micro: [0.0, 0.0],
            next_micro,
            pupil: 0.5,
            hippus,
            breath_phase: 0.0,
            breath_depth: 0.8,
            breath_pace,
            breath_size,
            roll,
            flare_prev: 0.0,
            kick: 0.0,
            wince: 0.0,
            last: Moved::default(),
        }
    }

    /// Advances the body `dt` seconds toward what the mind wants this frame.
    pub fn update(&mut self, dt: f64, s: &Frame) -> Moved {
        // A frame of no time (or a clock that ran backwards) can't move
        // anything, and dividing by it would make NaNs: show the last frame.
        if dt <= 0.0 || !dt.is_finite() {
            return self.last;
        }
        let dt = dt.min(0.1);
        if !self.started {
            self.started = true;
            self.gaze = [Spring::new(s.gaze.x), Spring::new(s.gaze.y)];
            self.pupil = s.pupil;
            self.flare_prev = s.flare;
            self.feel = Feel::for_mode(s.mode);
        }

        // The spring's feel eases toward the mode's over a third of a
        // second, so a change of mode never jerks the eye.
        let want = Feel::for_mode(s.mode);
        self.feel = Feel {
            f: approach(self.feel.f, want.f, 3.0, dt),
            zeta: approach(self.feel.zeta, want.zeta, 3.0, dt),
            r: approach(self.feel.r, want.r, 3.0, dt),
        };

        // Holding still: slow drift away from the point, and every second or
        // two a tiny quick jump (a microsaccade) to a fresh spot near it.
        // Both are far smaller than anything it aims at; the spring turns
        // each jump into a little flick.
        self.next_micro -= dt;
        if self.next_micro <= 0.0 {
            let ang = self.rng.f64() * TAU;
            let r = MICRO * self.rng.f64().sqrt();
            self.micro = [ang.cos() * r, ang.sin() * r];
            // Fixing on a face, it corrects more often than when it roams.
            let (lo, span) = if s.mode == Mode::Locked {
                (0.5, 1.2)
            } else {
                (0.8, 2.0)
            };
            self.next_micro = lo + self.rng.f64() * span;
        }
        let mut gaze = [0.0; 2];
        let mut gaze_vel = [0.0; 2];
        let aim = [s.gaze.x, s.gaze.y];
        for i in 0..2 {
            let wobble = self.drift[i].step(dt, &mut self.rng) * DRIFT + self.micro[i];
            gaze[i] = self.gaze[i].step(aim[i] + wobble, self.feel, dt) as f32;
            gaze_vel[i] = self.gaze[i].v as f32;
        }

        // The pupil eases after the mind's (which may snap shut on a bang),
        // and never sits quite still.
        let hippus = 1.0 + HIPPUS * self.hippus.step(dt, &mut self.rng);
        let before = self.pupil;
        self.pupil = approach(self.pupil, s.pupil * hippus, 9.0, dt).max(0.0);
        let pupil_vel = (self.pupil - before) / dt;

        // Breath. Worked up (hot, flaring) it breathes faster and deeper;
        // dim and drowsy, slower and shallower. The pace and depth wobble
        // a little on their own too.
        let excite = (s.heat * 0.6 + s.flare).clamp(0.0, 1.0);
        let drowsy = s.dim.clamp(0.0, 1.0);
        let pace = self.breath_pace.step(dt, &mut self.rng);
        let size = self.breath_size.step(dt, &mut self.rng);
        let secs = BREATH_SECS * (1.0 + 0.15 * pace) * (1.0 - 0.35 * excite) * (1.0 + 0.4 * drowsy);
        self.breath_phase = (self.breath_phase + dt / secs).fract();
        let depth = (0.75 + 0.15 * size + 0.25 * excite - 0.3 * drowsy).clamp(0.2, 1.0);
        self.breath_depth = approach(self.breath_depth, depth, 1.0, dt);
        // In is quicker than out: the first 40% of the cycle inhales.
        let p = self.breath_phase;
        let warped = if p < 0.4 {
            p / 0.4 * 0.5
        } else {
            0.5 + (p - 0.4) / 0.6 * 0.5
        };
        let breath = (-(warped * TAU).cos() * self.breath_depth).clamp(-1.0, 1.0);

        // Wince. A flare that jumps in one frame is a startle: the kick
        // fades in about a quarter second, and the wince follows it, fast
        // on the way up and slow on the way down.
        let jump = s.flare - self.flare_prev;
        self.flare_prev = s.flare;
        if jump > STARTLE_JUMP {
            self.kick = self.kick.max(jump.min(1.0));
        }
        self.kick = approach(self.kick, 0.0, 4.0, dt);
        let rate = if self.kick > self.wince { 35.0 } else { 2.5 };
        self.wince = approach(self.wince, self.kick, rate, dt);
        // An exponential only ever nears 0; snap the last crumb so the
        // sphere really is perfect again at rest.
        if self.wince < 1e-4 && self.kick < 1e-4 {
            self.wince = 0.0;
            self.kick = 0.0;
        }

        let roll = self.roll.step(dt, &mut self.rng) * ROLL;

        self.last = Moved {
            gaze,
            gaze_vel,
            pupil: self.pupil as f32,
            pupil_vel: pupil_vel as f32,
            heat: (s.heat * (1.0 + BREATH_HEAT * breath)) as f32,
            breath: breath as f32,
            wince: self.wince as f32,
            morph: (MORPH_WINCE * self.wince) as f32,
            roll: roll as f32,
            quality: 1.0,
        };
        self.last
    }
}

// The tests only exist under `cargo test`.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::mind::Vec2;

    fn frame(x: f64, y: f64, mode: Mode) -> Frame {
        Frame {
            gaze: Vec2::new(x, y),
            pupil: 0.5,
            heat: 0.4,
            flare: 0.0,
            wide: 0.5,
            dim: 0.0,
            shake: Vec2::default(),
            mode,
        }
    }

    fn motion() -> Motion {
        Motion::with_rng(fastrand::Rng::with_seed(7))
    }

    /// Runs `seconds` of frames at `dt`, calling `each` on every result.
    fn run(
        m: &mut Motion,
        f: &Frame,
        dt: f64,
        seconds: f64,
        mut each: impl FnMut(&Moved),
    ) -> Moved {
        let mut last = m.update(dt, f);
        each(&last);
        for _ in 1..(seconds / dt).round() as usize {
            last = m.update(dt, f);
            each(&last);
        }
        last
    }

    fn finite(o: &Moved) -> bool {
        [
            o.gaze[0],
            o.gaze[1],
            o.gaze_vel[0],
            o.gaze_vel[1],
            o.pupil,
            o.pupil_vel,
            o.heat,
            o.breath,
            o.wince,
            o.morph,
            o.roll,
            o.quality,
        ]
        .iter()
        .all(|v| v.is_finite())
    }

    #[test]
    fn gaze_converges_to_a_step_at_any_frame_rate() {
        for dt in [1.0 / 240.0, 1.0 / 60.0, 0.1] {
            for mode in [Mode::Locked, Mode::Hunting, Mode::Blind] {
                let mut m = motion();
                run(&mut m, &frame(0.0, 0.0, mode), dt, 1.0, |_| {});
                let end = run(&mut m, &frame(0.4, -0.3, mode), dt, 4.0, |o| {
                    assert!(finite(o), "NaN at dt={dt} {mode:?}: {o:?}")
                });
                // Within the fixational wobble of the aim.
                assert!(
                    (end.gaze[0] - 0.4).abs() < 0.02,
                    "dt={dt} {mode:?}: {end:?}"
                );
                assert!(
                    (end.gaze[1] + 0.3).abs() < 0.02,
                    "dt={dt} {mode:?}: {end:?}"
                );
            }
        }
    }

    #[test]
    fn overshoot_is_bounded() {
        for dt in [1.0 / 240.0, 1.0 / 60.0, 0.1] {
            for mode in [Mode::Locked, Mode::Hunting, Mode::Blind] {
                let mut m = motion();
                run(&mut m, &frame(0.0, 0.0, mode), dt, 1.0, |_| {});
                let mut peak = f32::MIN;
                run(&mut m, &frame(0.5, 0.0, mode), dt, 3.0, |o| {
                    peak = peak.max(o.gaze[0])
                });
                // A pure step is the worst case (the mind's own gaze is
                // already smoothed); even then, no more than a quarter past.
                eprintln!(
                    "overshoot dt={dt:.4} {mode:?}: {:.1}%",
                    (peak / 0.5 - 1.0) * 100.0
                );
                assert!(peak < 0.5 * 1.25, "dt={dt} {mode:?}: peak {peak}");
            }
        }
    }

    #[test]
    fn a_fast_turn_swings_a_little_past() {
        let mut m = motion();
        run(
            &mut m,
            &frame(0.0, 0.0, Mode::Locked),
            1.0 / 60.0,
            1.0,
            |_| {},
        );
        let mut peak = f32::MIN;
        run(
            &mut m,
            &frame(0.5, 0.0, Mode::Locked),
            1.0 / 60.0,
            2.0,
            |o| peak = peak.max(o.gaze[0]),
        );
        assert!(peak > 0.5 + 0.01, "no overshoot: {peak}");
    }

    #[test]
    fn breath_stays_in_range() {
        let mut m = motion();
        let mut f = frame(0.0, 0.0, Mode::Hunting);
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for (heat, flare, dim) in [(0.2, 0.0, 0.0), (1.0, 1.0, 0.0), (0.0, 0.0, 1.0)] {
            f.heat = heat;
            f.flare = flare;
            f.dim = dim;
            for dt in [1.0 / 240.0, 0.1] {
                run(&mut m, &f, dt, 20.0, |o| {
                    assert!((-1.0..=1.0).contains(&o.breath), "{o:?}");
                    lo = lo.min(o.breath);
                    hi = hi.max(o.breath);
                });
            }
        }
        // And it actually breathes.
        assert!(lo < -0.3 && hi > 0.3, "{lo}..{hi}");
    }

    #[test]
    fn a_startle_winces_then_settles_to_a_perfect_sphere() {
        let mut m = motion();
        let calm = frame(0.0, 0.0, Mode::Locked);
        let rest = run(&mut m, &calm, 1.0 / 60.0, 1.0, |_| {});
        assert_eq!(rest.wince, 0.0);
        assert_eq!(rest.morph, 0.0);

        // A bang: the mind's flare leaps to 1, then decays as the mind's does.
        let mut f = calm;
        f.flare = 1.0;
        let mut peak = 0.0f32;
        for _ in 0..120 {
            let o = m.update(1.0 / 60.0, &f);
            peak = peak.max(o.wince);
            f.flare = (f.flare - 0.9 / 60.0).max(0.0);
        }
        assert!(peak > 0.5, "wince peak {peak}");

        let end = run(&mut m, &calm, 1.0 / 60.0, 6.0, |_| {});
        assert_eq!(end.wince, 0.0);
        assert_eq!(end.morph, 0.0);
    }

    #[test]
    fn roll_and_pupil_stay_small_and_sane() {
        let mut m = motion();
        run(
            &mut m,
            &frame(0.0, 0.0, Mode::Hunting),
            1.0 / 60.0,
            60.0,
            |o| {
                assert!(o.roll.abs() <= ROLL as f32 + 1e-6, "{o:?}");
                assert!((o.pupil - 0.5).abs() < 0.5 * 0.05, "{o:?}");
                assert_eq!(o.morph, 0.0);
            },
        );
    }

    #[test]
    fn a_seeded_motion_repeats_itself() {
        let f = frame(0.2, 0.1, Mode::Hunting);
        let mut a = Vec::new();
        let mut b = Vec::new();
        run(&mut motion(), &f, 1.0 / 60.0, 5.0, |o| a.push(*o));
        run(&mut motion(), &f, 1.0 / 60.0, 5.0, |o| b.push(*o));
        assert_eq!(a, b);
    }

    #[test]
    fn a_zero_frame_changes_nothing() {
        let mut m = motion();
        let f = frame(0.1, 0.1, Mode::Locked);
        let o = run(&mut m, &f, 1.0 / 60.0, 1.0, |_| {});
        assert_eq!(m.update(0.0, &f), o);
    }
}
