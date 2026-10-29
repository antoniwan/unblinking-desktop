//! One frame of the eye: senses -> mind -> feeling -> motion -> uniforms,
//! and what Blinki's Senses shows of it.

use std::time::{Duration, Instant};

use winit::window::Window;

use super::*;
use crate::desktop::{self};
use crate::eye::EyeState;
use crate::feeling::{self};
use crate::mind::{self, Senses};
use crate::remembrance::{self};
use crate::senses::mic::Ears;
use crate::senses_panel::{LEVEL_HISTORY, Snapshot};
use crate::voice::Effect;
use crate::wellbeing::{self};

impl App {
    /// Advances the mind one frame and turns its mood into the eye's uniforms.
    pub(super) fn step(&mut self, window: &Window) -> EyeState {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f64().min(0.1);
        self.last_frame = now;

        // What the camera saw and the mic heard last.
        let (faces, can_see) = match &mut self.eyes {
            Some(eyes) => {
                let sight = eyes.look();
                let faces: Vec<_> = sight.detections.iter().map(|d| d.face).collect();
                self.expression = sight.expression;
                (faces, sight.can_see)
            }
            None => (vec![], false),
        };
        // The mic hears the speakers, so it goes deaf while the eye speaks.
        let heard = if let Some(demo) = &self.demo {
            demo.level()
        } else if self.voice.speaking() {
            0.0
        } else {
            self.ears.as_ref().map_or(0.0, Ears::level)
        };
        // In --demo, the cursor follows the script.
        let mut world = self.world(window);
        if let (Some(demo), Some(w)) = (&self.demo, &mut world) {
            w.cursor = demo.cursor(w.eye, w.scale);
        }
        let s = self.mind.update(
            dt,
            &Senses {
                faces: &faces,
                can_see,
                level: heard,
                world,
                blind: self.prefs.blind && self.demo.is_none(),
                // In --demo the script does the talking.
                whispers: self.prefs.whispers && self.demo.is_none(),
            },
        );

        // Debug heartbeat in the terminal, as app.js had: what it sees.
        self.frames += 1;
        if self.last_beat.elapsed() >= Duration::from_secs(3) {
            let fps = self.frames as f64 / self.last_beat.elapsed().as_secs_f64();
            self.frames = 0;
            self.last_beat = now;
            for (i, f) in faces.iter().enumerate().skip(1) {
                eprintln!(
                    "  also face {i}: ({:.2},{:.2}) size={:.2} score={:.2}",
                    f.x, f.y, f.size, f.score
                );
            }
            let first = faces
                .first()
                .map(|f| {
                    format!(
                        " face=({:.2},{:.2}) size={:.2} score={:.2}",
                        f.x, f.y, f.size, f.score
                    )
                })
                .unwrap_or_default();
            let level = self.ears.as_ref().map(Ears::level);
            eprintln!(
                "mode={:?} mood={} sight={can_see} faces={}{first} level={level:.2?} gaze=({:.2},{:.2}) fps={fps:.0}",
                s.mode,
                self.moods.current().name,
                faces.len(),
                s.gaze.x,
                s.gaze.y
            );
        }

        // A line in the terminal whenever the mood changes.
        if self.mode != Some(s.mode) {
            eprintln!("mode: {:?}", s.mode);
            self.mode = Some(s.mode);
        }

        // Speak the newest whisper, if the voice is on; follow the mood's heat.
        let latest = {
            let log = self.whisper_log.borrow();
            let fresh = log.len() > self.spoken;
            self.spoken = log.len();
            fresh
                .then(|| log.last().map(|(_, _, text)| *text))
                .flatten()
        };
        if let Some(text) = latest {
            if self.prefs.whispers {
                self.pending_whisper = Some(text.to_string());
            }
            self.voice.say(text);
        }
        self.voice.update(&s, dt as f32);
        // A new startle: the flinch effect.
        let startle = self.mind.insight().last_startle;
        if startle > self.last_flinch_sound {
            self.last_flinch_sound = startle;
            self.voice.effect(Effect::Flinch);
        }

        // The mood: which of the studio's looks fits what's happening.
        let level = heard;
        if !faces.is_empty() {
            self.last_face = Some(now);
        }
        if level > 0.3 {
            self.last_noise = now;
        }
        // Your face, if you let it read it (None otherwise).
        let expression = self.expression;

        // What it remembers: notice good deeds, and whisper any first.
        if let Some(book) = &mut self.book {
            let firsts = self.watch.update(
                &wellbeing::Signals {
                    now: remembrance::unix_now(),
                    dt: dt as f32,
                    face: !faces.is_empty(),
                    idle_secs: desktop::idle_secs(),
                    hour: desktop::local_hour(),
                    expression,
                },
                book,
            );
            if let Some(first) = firsts.into_iter().next() {
                eprintln!("remembered a first: {}", first.line);
                if self.prefs.whispers {
                    self.voice.say(&first.line);
                    self.pending_whisper = Some(first.line);
                }
            }
        }
        // Up late: time here (face, or input in the last 30 s) after
        // midnight. Five minutes away, or morning, starts it over.
        let here = !faces.is_empty() || desktop::idle_secs().is_some_and(|i| i < 30.0);
        if here {
            self.last_here = now;
        }
        let small_hours = desktop::local_hour() < LATE_UNTIL_HOUR;
        if !small_hours || self.last_here.elapsed() > Duration::from_secs(300) {
            self.up_late = 0.0;
        } else if here {
            self.up_late += dt as f32;
        }

        let now_unix = remembrance::unix_now();
        let temperament = self.book.as_ref().map(|b| b.temperament(now_unix));

        let choice = self.feeling.update(
            dt as f32,
            &feeling::Context {
                mode: s.mode,
                last_said: self
                    .last_said
                    .get()
                    .map(|(kind, at)| (kind, at.elapsed().as_secs_f32())),
                no_face_secs: self
                    .last_face
                    .map_or(self.started.elapsed(), |t| t.elapsed())
                    .as_secs_f32(),
                hour: desktop::local_hour(),
                quiet_secs: self.last_noise.elapsed().as_secs_f32(),
                too_close: faces.iter().any(|f| f.score > 0.55 && f.size > 0.5),
                crowd: mind::crowd_size(&faces),
                temperament,
                expression,
                up_late_secs: self.up_late,
            },
        );
        // Worried: tell you to rest, gently, at most twice an hour.
        // It gets more insistent the later it gets (feeling::worry).
        let worry = feeling::worry(self.up_late);
        if choice.mood == "Worried"
            && self
                .last_worry
                .is_none_or(|t| t.elapsed().as_secs_f32() > worry.every_secs)
        {
            self.last_worry = Some(now);
            let line = worry.lines[fastrand::usize(..worry.lines.len())];
            eprintln!("worried: {line}");
            if self.prefs.whispers {
                self.voice.say(line);
                self.pending_whisper = Some(line.to_string());
            }
        }
        // `as_deref` turns `Option<String>` into `Option<&str>` without copying.
        let sulk = self.fullscreen.sulk().filter(|_| !self.prefs.blind);
        let (mood, reason) = match (self.args.mood.as_deref(), &self.demo, sulk) {
            (Some(m), _, _) => (m, "held by --mood"),
            (None, Some(demo), _) => (demo.mood(), "the --demo script"),
            (None, None, Some(m)) => (m, "sulking after a full-screen app"),
            (None, None, None) => (choice.mood, choice.reason),
        };
        // The script's whispers, unless a mood is held (stills for the README).
        let script_talks = self.args.mood.is_none();
        if let Some(text) = self
            .demo
            .as_mut()
            .filter(|_| script_talks)
            .and_then(|d| d.whisper())
        {
            match text.1 {
                // Logged like the mind's own, so Blinki's Senses lists it;
                // the log puts it on screen (and speaks it) next frame.
                Some(kind) => self
                    .whisper_log
                    .borrow_mut()
                    .push((Instant::now(), kind, text.0)),
                None => self.pending_whisper = Some(text.0.to_string()),
            }
        }
        self.moods.set(mood);
        let mut look = self.moods.now();
        if let (Some(t), Some(book)) = (temperament, &self.book) {
            // Remembrance's day runs 0.45 (left alone) to 1 (well cared for);
            // tint wants 0..1 with 0.5 an ordinary day.
            let day = (book.day_brightness(now_unix) - 0.45) / 0.55 * 0.5 + 0.25;
            look = feeling::tint(look, &t, day);
        }
        if mood == "Worried" {
            look.shake += worry.tremble;
        }
        self.flow = (self.flow + dt as f32 * look.flow) % 1.0;

        // The mind's frame, given a body: a gaze with weight and drift, a
        // restless pupil, breath, and a flinch (see motion.rs).
        let m = self.motion.update(dt, &s);

        // Opening, the eye grows and fades in with its pupil wide, then the
        // pupil settles to what the mind wants. Closing, it narrows, cools,
        // shrinks and fades to nothing.
        let k = self.presence.amount();
        let closing = matches!(self.presence, Presence::Closing(..));
        let pupil = m.pupil;
        let pupil = if closing {
            pupil * k
        } else {
            1.0 + (pupil - 1.0) * k
        };
        let cool = if closing { k } else { 1.0 };

        // The mind's numbers carry the moment; the mood scales them. Heat is
        // relative to Watchful's 0.55, so Watchful leaves it as the mind set it.
        let jitter = (look.shake + s.flare as f32 * 0.5) * 0.006;
        let shake = [
            s.shake.x as f32 + (fastrand::f32() - 0.5) * jitter,
            s.shake.y as f32 + (fastrand::f32() - 0.5) * jitter,
        ];
        let state = EyeState {
            flow: self.flow,
            gaze: m.gaze,
            pupil,
            heat: m.heat * look.heat / 0.55 * cool,
            flare: s.flare as f32,
            wide: s.wide as f32 * look.wake * cool,
            dim: (s.dim as f32).max(look.dim),
            center: [CENTER[0] + shake[0], CENTER[1] + shake[1]],
            fade: k,
            size: 0.75 + 0.25 * k,
            colors: look.display_colors(),
            swirl: look.swirl,
            glow: look.glow,
            embers: look.embers,
            time: (self.started.elapsed().as_secs_f32()) % 3600.0,
            breath: m.breath,
            morph: m.morph,
            gaze_vel: m.gaze_vel,
            pupil_vel: m.pupil_vel,
            quality: m.quality,
            wince: m.wince,
            roll: m.roll,
        };
        if self.panel.is_some() {
            self.observe(&s, &state, reason, level, window);
        }
        state
    }

    /// Gathers what Blinki's Senses shows this frame.
    pub(super) fn observe(
        &mut self,
        s: &mind::Frame,
        state: &EyeState,
        reason: &'static str,
        level: f64,
        window: &Window,
    ) {
        let insight = self.mind.insight();
        // A new startle time means it flinched this frame.
        let flinched = insight.last_startle > self.last_startle;
        self.last_startle = insight.last_startle;
        self.levels.push_back(level as f32);
        self.flinch_marks.push_back(flinched);
        while self.levels.len() > LEVEL_HISTORY {
            self.levels.pop_front();
            self.flinch_marks.pop_front();
        }
        let (detections, can_see, picture) = match &mut self.eyes {
            Some(eyes) => {
                let sight = eyes.look();
                (
                    sight.detections.clone(),
                    sight.can_see,
                    sight.preview.clone(),
                )
            }
            None => (vec![], false, None),
        };
        // Where "sitting normally" lands in the picture, at the tracked face's distance.
        let rest_in_picture = insight.tracked.map(|f| {
            let off = self.prefs.look_offset.map_or((0.0, 0.0), |p| (p.x, p.y));
            let [_, _, dist] = mind::face_in_room(&f);
            let tan_h = (70.0f64.to_radians() / 2.0).tan();
            let tan_v = tan_h / f.aspect;
            let x = 0.5 - (mind::REST_SEAT.x + off.0) / (2.0 * tan_h * dist);
            let y = 0.5 + (mind::REST_SEAT.y + off.1) / (2.0 * tan_v * dist);
            [x as f32, y as f32]
        });
        let world = self.world(window);
        let whispers = self
            .whisper_log
            .borrow()
            .iter()
            .map(|(at, kind, text)| (insight.t - at.elapsed().as_secs_f64(), *kind, *text))
            .collect();
        self.snapshot = Some(Snapshot {
            frame: *s,
            drawn_gaze: state.gaze,
            mood: self.moods.current().name.clone(),
            mood_reason: reason,
            mood_blend: self.moods.blend(),
            colors: state.colors,
            detections,
            can_see,
            blind: self.prefs.blind && self.demo.is_none(),
            demo: self.demo.is_some(),
            picture,
            rest_in_picture,
            levels: self.levels.clone(),
            flinches: self
                .flinch_marks
                .iter()
                .enumerate()
                .filter(|(_, f)| **f)
                .map(|(i, _)| i)
                .collect(),
            heard: match &self.demo {
                Some(demo) => Some(demo.level()),
                None => self.ears.as_ref().map(Ears::level),
            },
            whispers,
            cursor: world.map(|w| w.cursor),
            eye: world.map(|w| w.eye),
            display: world.map(|w| w.display),
            idle: desktop::idle_secs(),
            clock: desktop::local_time(),
            sound: self.prefs.sound,
            camera: self.prefs.camera.as_ref().map_or_else(
                || "top middle of the main screen (default)".to_string(),
                |c| c.describe(),
            ),
            remembers: self.book.as_ref().map(|b| {
                let traits = b.traits().into_iter().map(|t| t.name()).collect();
                (traits, b.journal().last().map(|e| e.line.clone()))
            }),
            expression: self.expression,
            insight,
        });
    }
}
