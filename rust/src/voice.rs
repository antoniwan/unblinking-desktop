//! The voice: a low drone under the fire, and whispers spoken aloud. The
//! Rust side of renderer/voice.js. The drone is synthesised here, sample by
//! sample; speech uses the system's own local voices. No audio files,
//! nothing fetched.
//!
//! The drone is quiet at rest and swells (louder, brighter) with heat and
//! flare. The mic hears the speakers, so the drone stays low and ducks
//! while it speaks; see `speaking`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::mind::{Frame, Mode};

// Drone loudness at rest and at full burn. Kept well below what the mic calls talking.
const REST_GAIN: f32 = 0.025;
const PEAK_GAIN: f32 = 0.11;
// Filter cutoff in Hz, closed at rest, open at full burn.
const REST_CUTOFF: f32 = 140.0;
const PEAK_CUTOFF: f32 = 900.0;
// The room keeps ringing a moment after the voice stops.
const SPEECH_TAIL: Duration = Duration::from_millis(600);
// Fade out this long before letting the speakers go, so it doesn't click.
const FADE_OUT: Duration = Duration::from_millis(300);

// Deep, plain voices first; bright ones last. Names vary by OS.
const DEEP: [&str; 11] = [
    "david", "mark", "guy", "george", "daniel", "fred", "ralph", "bruce", "alex", "thomas", "male",
];
const BRIGHT: [&str; 12] = [
    "zira", "hazel", "susan", "samantha", "victoria", "karen", "moira", "tessa", "female",
    "whisper", "bells", "bubbles",
];

/// The drone's controls, shared with the audio thread. Each is an f32
/// stored as bits in an AtomicU32: the main thread writes targets, the
/// audio thread glides toward them, and nobody ever waits on a lock.
struct Controls([AtomicU32; 5]);

#[derive(Clone, Copy)]
enum Knob {
    Gain,
    Cutoff,
    Hiss,
    Band,
    Breath,
}

impl Controls {
    fn new() -> Controls {
        let c = Controls(Default::default());
        c.set(Knob::Gain, 0.0);
        c.set(Knob::Cutoff, REST_CUTOFF);
        c.set(Knob::Hiss, 0.15);
        c.set(Knob::Band, 220.0);
        c.set(Knob::Breath, 0.07);
        c
    }
    fn set(&self, k: Knob, v: f32) {
        self.0[k as usize].store(v.to_bits(), Ordering::Relaxed);
    }
    fn get(&self, k: Knob) -> f32 {
        f32::from_bits(self.0[k as usize].load(Ordering::Relaxed))
    }
}

/// A two-pole filter (the "biquad" Web Audio uses), in direct form I.
#[derive(Default)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    x: [f32; 2],
    y: [f32; 2],
}

impl Biquad {
    /// Low-pass or band-pass coefficients from the Audio EQ Cookbook.
    fn tune(&mut self, low_pass: bool, freq: f32, q: f32, rate: f32) {
        let w = std::f32::consts::TAU * freq.clamp(10.0, rate * 0.45) / rate;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q);
        let a0 = 1.0 + alpha;
        self.b = if low_pass {
            [(1.0 - c) / 2.0 / a0, (1.0 - c) / a0, (1.0 - c) / 2.0 / a0]
        } else {
            [alpha / a0, 0.0, -alpha / a0]
        };
        self.a = [-2.0 * c / a0, (1.0 - alpha) / a0];
    }

    fn run(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1]
            - self.a[0] * self.y[0]
            - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// Everything the audio thread owns. Nothing here allocates once running:
/// the callback must never wait on memory or locks, or the sound stutters.
struct Synth {
    rate: f32,
    controls: Arc<Controls>,
    // The current (smoothed) value of each knob.
    now: [f32; 5],
    phases: [f32; 4],
    breath_phase: f32,
    low: Biquad,
    band: Biquad,
    brown: f32,
    seed: u32,
    until_retune: u32,
}

/// The oscillators: a root, a slightly sour twin, a sub, a faint fifth,
/// as (frequency, cents of detune, gain, shape).
const OSCS: [(f32, f32, f32, Shape); 4] = [
    (55.0, 0.0, 0.35, Shape::Saw),
    (55.0, 11.0, 0.3, Shape::Saw),
    (27.5, -4.0, 0.5, Shape::Triangle),
    (82.4, 6.0, 0.08, Shape::Sine),
];

#[derive(Clone, Copy)]
enum Shape {
    Saw,
    Triangle,
    Sine,
}

/// How long each knob takes to glide (Web Audio's setTargetAtTime constants).
const GLIDE: [f32; 5] = [0.12, 0.15, 0.2, 0.2, 1.5];

impl Synth {
    fn new(rate: f32, controls: Arc<Controls>) -> Synth {
        let now = [0, 1, 2, 3, 4].map(|i| f32::from_bits(controls.0[i].load(Ordering::Relaxed)));
        Synth {
            rate,
            controls,
            now,
            phases: [0.0; 4],
            breath_phase: 0.0,
            low: Biquad::default(),
            band: Biquad::default(),
            brown: 0.0,
            seed: 0x9e37_79b9,
            until_retune: 0,
        }
    }

    /// One sample of the drone.
    fn next(&mut self) -> f32 {
        // Retune the filters every 64 samples; that's plenty for slow glides.
        if self.until_retune == 0 {
            self.until_retune = 64;
            for (i, k) in [
                Knob::Gain,
                Knob::Cutoff,
                Knob::Hiss,
                Knob::Band,
                Knob::Breath,
            ]
            .into_iter()
            .enumerate()
            {
                let step = 1.0 - (-64.0 / (GLIDE[i] * self.rate)).exp();
                self.now[i] += (self.controls.get(k) - self.now[i]) * step;
            }
            // A slow breath on the filter, so the drone never sits perfectly still.
            let breath = (self.breath_phase * std::f32::consts::TAU).sin() * 40.0;
            self.low.tune(
                true,
                self.now[Knob::Cutoff as usize] + breath,
                4.0,
                self.rate,
            );
            self.band
                .tune(false, self.now[Knob::Band as usize], 0.8, self.rate);
        }
        self.until_retune -= 1;
        self.breath_phase = (self.breath_phase + self.now[Knob::Breath as usize] / self.rate) % 1.0;

        let mut tone = 0.0;
        for (phase, &(freq, cents, gain, shape)) in self.phases.iter_mut().zip(&OSCS) {
            let f = freq * 2f32.powf(cents / 1200.0);
            *phase = (*phase + f / self.rate) % 1.0;
            let p = *phase;
            tone += gain
                * match shape {
                    Shape::Saw => 2.0 * p - 1.0,
                    Shape::Triangle => 1.0 - 4.0 * (p - 0.5).abs(),
                    Shape::Sine => (p * std::f32::consts::TAU).sin(),
                };
        }
        // Brown noise: white noise, smoothed and leaking back to zero.
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        let white = self.seed as f32 / u32::MAX as f32 * 2.0 - 1.0;
        self.brown = (self.brown + white * 0.02) / 1.02;
        let hiss = self.band.run(self.brown * 3.5) * self.now[Knob::Hiss as usize];

        (self.low.run(tone) + hiss) * self.now[Knob::Gain as usize]
    }
}

/// The drone's output stream and the knobs that steer it.
struct Drone {
    _stream: cpal::Stream,
    controls: Arc<Controls>,
}

impl Drone {
    fn open() -> Result<Drone, String> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or("no speakers")?;
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        let channels = supported.channels() as usize;
        let controls = Arc::new(Controls::new());
        let mut synth = Synth::new(supported.sample_rate() as f32, Arc::clone(&controls));
        let on_error = |err| eprintln!("voice error: {err}");
        let config = supported.config();
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                config,
                // Same sample on every channel: the drone is mono.
                move |out: &mut [f32], _: &_| {
                    for frame in out.chunks_mut(channels) {
                        frame.fill(synth.next());
                    }
                },
                on_error,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_output_stream(
                config,
                move |out: &mut [i16], _: &_| {
                    for frame in out.chunks_mut(channels) {
                        frame.fill((synth.next().clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
                    }
                },
                on_error,
                None,
            ),
            other => return Err(format!("unsupported sample format {other:?}")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Drone {
            _stream: stream,
            controls,
        })
    }
}

/// The voice as the app sees it: drone plus speech, on or off together.
pub struct Voice {
    drone: Option<Drone>,
    speech: Option<tts::Tts>,
    enabled: bool,
    /// When the drone, having faded out, should let the speakers go.
    release_at: Option<Instant>,
    swell: f32,
    duck: f32,
    asked_to_speak: bool,
    quiet_since: Option<Instant>,
}

fn approach(cur: f32, target: f32, rate: f32, dt: f32) -> f32 {
    cur + (target - cur) * (1.0 - (-rate * dt).exp())
}

impl Voice {
    pub fn new() -> Voice {
        Voice {
            drone: None,
            speech: None,
            enabled: false,
            release_at: None,
            swell: 0.0,
            duck: 0.0,
            asked_to_speak: false,
            quiet_since: None,
        }
    }

    pub fn set_enabled(&mut self, on: bool) {
        if on == self.enabled {
            return;
        }
        self.enabled = on;
        if on {
            self.release_at = None;
            if self.drone.is_none() {
                self.drone = Drone::open()
                    .map_err(|err| eprintln!("no drone: {err}"))
                    .ok();
            }
            if self.speech.is_none() {
                self.speech = open_speech()
                    .map_err(|err| eprintln!("no speech: {err}"))
                    .ok();
            }
        } else {
            if let Some(tts) = &mut self.speech {
                let _ = tts.stop();
            }
            self.asked_to_speak = false;
            if let Some(d) = &self.drone {
                d.controls.set(Knob::Gain, 0.0);
            }
            self.release_at = Some(Instant::now() + FADE_OUT);
        }
    }

    /// Follows the mind's mood: swell with heat and flare, duck while speaking.
    pub fn update(&mut self, s: &Frame, dt: f32) {
        if self.release_at.is_some_and(|t| Instant::now() >= t) {
            // Faded out: dropping the stream lets the speakers go.
            self.drone = None;
            self.release_at = None;
        }
        // Notice when a spoken line has finished.
        if self.asked_to_speak && !self.engine_speaking() {
            self.asked_to_speak = false;
            self.quiet_since = Some(Instant::now());
        }
        if !self.enabled {
            return;
        }
        let Some(drone) = &self.drone else { return };
        let target =
            ((s.heat * 0.55 + s.flare * 0.8) as f32).clamp(0.0, 1.0) * (1.0 - s.dim as f32 * 0.75);
        // Swell up fast, sink back slowly.
        self.swell = approach(
            self.swell,
            target,
            if target > self.swell { 6.0 } else { 1.2 },
            dt,
        );
        let speaking = self.speaking();
        self.duck = approach(
            self.duck,
            if speaking { 1.0 } else { 0.0 },
            if speaking { 8.0 } else { 1.5 },
            dt,
        );

        let c = &drone.controls;
        let sw = self.swell;
        c.set(
            Knob::Gain,
            (REST_GAIN + (PEAK_GAIN - REST_GAIN) * sw * sw) * (1.0 - self.duck * 0.7),
        );
        c.set(
            Knob::Cutoff,
            REST_CUTOFF * (PEAK_CUTOFF / REST_CUTOFF).powf(sw),
        );
        c.set(Knob::Hiss, 0.15 + sw * 0.35);
        c.set(Knob::Band, 220.0 + sw * 500.0);
        // Hunting makes it breathe faster; blind makes it sleep.
        let breath = match s.mode {
            Mode::Hunting => 0.16,
            Mode::Blind => 0.04,
            Mode::Locked => 0.07,
        };
        c.set(Knob::Breath, breath);
    }

    /// Speaks a line in a low, slow voice. Never queues: if it's still
    /// talking, the line is skipped.
    pub fn say(&mut self, text: &str) -> bool {
        if !self.enabled || self.engine_speaking() {
            return false;
        }
        let Some(tts) = &mut self.speech else {
            return false;
        };
        match tts.speak(text, false) {
            Ok(_) => {
                // Duck from the moment it's asked, not when the engine gets around to it.
                self.asked_to_speak = true;
                true
            }
            Err(err) => {
                eprintln!("could not speak: {err}");
                false
            }
        }
    }

    /// True while a line is being spoken, plus a short tail for the room to
    /// go quiet. The mic hears the speakers, so loudness can't be trusted then.
    pub fn speaking(&self) -> bool {
        self.asked_to_speak || self.quiet_since.is_some_and(|t| t.elapsed() < SPEECH_TAIL)
    }

    fn engine_speaking(&self) -> bool {
        self.speech
            .as_ref()
            .is_some_and(|t| t.is_speaking().unwrap_or(false))
    }
}

/// The system speech engine with the deepest local voice it has, slowed
/// and lowered like voice.js did (Web Speech and this crate share scales).
fn open_speech() -> Result<tts::Tts, String> {
    let mut tts = tts::Tts::default().map_err(|e| e.to_string())?;
    if let Ok(voices) = tts.voices() {
        let score = |v: &tts::Voice| {
            let name = v.name().to_lowercase();
            let mut s = 0;
            if v.language().as_str().to_lowercase().starts_with("en") {
                s += 10;
            }
            if DEEP.iter().any(|n| name.contains(n)) || v.gender() == Some(tts::Gender::Male) {
                s += 5;
            }
            if BRIGHT.iter().any(|n| name.contains(n)) {
                s -= 5;
            }
            s
        };
        if let Some(best) = voices.iter().max_by_key(|v| score(v)) {
            eprintln!("voice: {}", best.name());
            let _ = tts.set_voice(best);
        }
    }
    let _ = tts.set_pitch(0.3_f32.max(tts.min_pitch()));
    let _ = tts.set_rate(0.72_f32.max(tts.min_rate()));
    let _ = tts.set_volume(0.6);
    Ok(tts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_drone_is_quiet_and_never_blows_up() {
        let controls = Arc::new(Controls::new());
        controls.set(Knob::Gain, PEAK_GAIN);
        controls.set(Knob::Cutoff, PEAK_CUTOFF);
        let mut synth = Synth::new(48_000.0, Arc::clone(&controls));
        let mut peak = 0.0f32;
        for _ in 0..48_000 * 2 {
            let s = synth.next();
            assert!(s.is_finite());
            peak = peak.max(s.abs());
        }
        assert!(peak > 0.005, "it makes a sound: {peak}");
        assert!(peak < 0.5, "but a quiet one: {peak}");
    }

    #[test]
    fn silent_until_turned_up() {
        let mut synth = Synth::new(48_000.0, Arc::new(Controls::new()));
        assert!((0..4800).all(|_| synth.next() == 0.0));
    }
}
