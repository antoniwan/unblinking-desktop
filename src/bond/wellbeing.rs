//! Wellbeing: the `Watch` that turns what the senses report each frame
//! into deeds for Remembrance (see the `remembrance` module, after
//! Lifesworn's Remembrance). The app (app/frame.rs) fills in a `Signals`
//! and calls `Watch::update` once a frame; that's all.
//!
//! What it looks at is deliberately small: whether a face is in view, how
//! long since the last keyboard or mouse input (never *which* keys), the
//! hour, and an optional smile reading. Nothing here is stored except the
//! deeds themselves, in the ledger, on this computer.
//!
//! The rules, in plain words:
//!
//! - **Here** means a face is in view, or you touched the keyboard or mouse
//!   in the last `INPUT_HERE_SECS`. Either is enough, so it still works
//!   with the camera off.
//! - **A break**: away at least `BREAK_AWAY_SECS` after at least
//!   `BREAK_AFTER_SECS` here. Counted when you come back, the moment the
//!   blinki can say "good".
//! - **A return**: back after at least `RETURN_AWAY_SECS` away. A face
//!   that flickers out for a second doesn't count.
//! - **Rest**: a gap of `REST_GAP_SECS` or more between being here, even
//!   across the app being closed. Most likely a night's sleep.
//! - **An early morning**: arriving rested between `EARLY_HOURS`.
//! - **A long return**: back after `LONG_AWAY_SECS` or more.
//! - **Time together**: every full hour here.
//! - **A smile**: smiling above `SMILE_LEVEL` for `SMILE_HOLD_SECS`, at
//!   most once per `SMILE_EVERY_SECS`.
//! - **A late night**: here during `LATE_HOURS`, once a night. It costs a
//!   little calm, the way a late night does; it's never a mark against you.

use std::ops::Range;

use crate::bond::remembrance::{Deed, Entry, Remembrance};
use crate::senses::expression::Expression;

/// Input this recent counts as being here, in seconds.
pub const INPUT_HERE_SECS: f64 = 30.0;
/// Away at least this long counts as a break (5 minutes)...
pub const BREAK_AWAY_SECS: u64 = 5 * 60;
/// ...after at least this long here (45 minutes), the usual advice for
/// screen work being a short break every 45 to 60 minutes.
pub const BREAK_AFTER_SECS: u64 = 45 * 60;
/// Away this long and coming back is a return (2 minutes): long enough to
/// have really gone somewhere, short enough to notice a coffee.
pub const RETURN_AWAY_SECS: u64 = 2 * 60;
/// At most one return every 10 minutes, so pacing in and out of view
/// isn't a flood of "you came back".
pub const RETURN_EVERY_SECS: u64 = 10 * 60;
/// A gap this long is rest (6 hours): a short night still counts.
pub const REST_GAP_SECS: u64 = 6 * 3600;
/// Rest, early mornings and late nights count at most this often (12 h),
/// so each is "once a night" without needing a calendar.
pub const ONCE_A_NIGHT_SECS: u64 = 12 * 3600;
/// Gone this long is a long return (3 days).
pub const LONG_AWAY_SECS: u64 = 3 * 24 * 3600;
/// Local hours, start included and end not, for early mornings (5 to 9)...
pub const EARLY_HOURS: Range<u32> = 5..9;
/// ...and late nights (1 am to 5 am). Midnight itself is fair game.
pub const LATE_HOURS: Range<u32> = 1..5;
/// A smile above this, held this long, read with at least this much
/// confidence, at most this often.
pub const SMILE_LEVEL: f32 = 0.6;
pub const SMILE_HOLD_SECS: f32 = 1.0;
pub const SMILE_CONFIDENCE: f32 = 0.4;
pub const SMILE_EVERY_SECS: u64 = 10 * 60;
/// Seconds in the "time together" unit.
const HOUR_SECS: f64 = 3600.0;
/// The longest frame we believe. After the computer sleeps, the first
/// frame's `dt` can be hours long; that isn't time spent together (the
/// gap shows up as rest or a return instead, from the clock).
const MAX_DT: f32 = 1.0;

/// What the app knows this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Signals {
    /// Unix seconds, from `remembrance::unix_now()`.
    pub now: u64,
    /// Seconds since the last frame.
    pub dt: f32,
    /// A face is in view.
    pub face: bool,
    /// Seconds since the last keyboard or mouse input, from
    /// `desktop::idle_secs()`. `None` where the OS won't say.
    pub idle_secs: Option<f64>,
    /// Local hour, 0..23, from `desktop::local_hour()`.
    pub hour: u32,
    /// The face's expression, when the expression reader is on.
    pub expression: Option<Expression>,
}

/// Watches the frames go by and notices deeds. Holds only what it needs
/// between frames; everything lasting lives in the `Remembrance`.
#[derive(Debug, Default)]
pub struct Watch {
    /// Were you here last frame?
    here: bool,
    /// When this stretch at the screen began (reset by any absence long
    /// enough to be a break).
    stretch_start: Option<u64>,
    /// Seconds here toward the next full hour together.
    together: f64,
    /// How long the current smile has been held.
    smile_held: f32,
    /// This smile already counted (or was rate-limited); wait for it to end.
    smile_counted: bool,
}

impl Watch {
    pub fn new() -> Watch {
        Watch::default()
    }

    /// One frame. Records any deeds into `book`, saves it now and then,
    /// and returns the journal entries for any firsts: usually none, so
    /// the `Vec` is empty and costs nothing (an empty `Vec` doesn't
    /// allocate). If there are several, whisper the first; they're in
    /// order of how much they matter.
    pub fn update(&mut self, s: &Signals, book: &mut Remembrance) -> Vec<Entry> {
        let mut deeds: Vec<Deed> = Vec::new();
        let here = s.face || s.idle_secs.is_some_and(|i| i < INPUT_HERE_SECS);

        if here {
            // How long since you were last here, even across restarts.
            // `map` on an Option runs only when there's a value inside.
            let gap = book.last_seen().map(|t| s.now.saturating_sub(t));
            // Arriving: you weren't here last frame, or the clock jumped
            // (the computer slept with you in front of it).
            let arriving = !self.here || gap.is_some_and(|g| g >= RETURN_AWAY_SECS);
            if arriving {
                self.arrive(gap, s, book, &mut deeds);
            }
            book.mark_seen(s.now);
            self.keep_company(s, book, &mut deeds);
        } else {
            self.smile_held = 0.0;
            self.smile_counted = false;
        }
        self.here = here;

        let firsts = deeds
            .into_iter()
            .filter_map(|deed| book.record(deed, s.now))
            .collect();
        book.save_if_due(s.now);
        firsts
    }

    /// You just showed up, after `gap` seconds (`None`: never seen before).
    fn arrive(&mut self, gap: Option<u64>, s: &Signals, book: &Remembrance, deeds: &mut Vec<Deed>) {
        let since = |deed: Deed| since(book, deed, s.now);
        let Some(gap) = gap else {
            // The very first meeting. Nothing to count yet.
            self.stretch_start = Some(s.now);
            return;
        };
        // How long the last stretch lasted, up to when you left.
        let stretch = match (self.stretch_start, book.last_seen()) {
            (Some(start), Some(left)) => left.saturating_sub(start),
            _ => 0,
        };

        if gap >= LONG_AWAY_SECS {
            deeds.push(Deed::LongReturn);
        }
        if gap >= REST_GAP_SECS {
            if since(Deed::Rest) >= ONCE_A_NIGHT_SECS {
                deeds.push(Deed::Rest);
            }
            if EARLY_HOURS.contains(&s.hour) && since(Deed::EarlyMorning) >= ONCE_A_NIGHT_SECS {
                deeds.push(Deed::EarlyMorning);
            }
        } else if gap >= BREAK_AWAY_SECS && stretch >= BREAK_AFTER_SECS {
            deeds.push(Deed::Break);
        }
        if gap >= RETURN_AWAY_SECS && since(Deed::Return) >= RETURN_EVERY_SECS {
            deeds.push(Deed::Return);
        }

        // Away long enough for a break (taken or not): a new stretch begins.
        // A shorter absence just pauses the old one.
        if gap >= BREAK_AWAY_SECS || self.stretch_start.is_none() {
            self.stretch_start = Some(s.now);
        }
    }

    /// Every frame you're here: hours together, smiles, late nights.
    fn keep_company(&mut self, s: &Signals, book: &Remembrance, deeds: &mut Vec<Deed>) {
        let dt = s.dt.clamp(0.0, MAX_DT);

        self.together += dt as f64;
        if self.together >= HOUR_SECS {
            self.together -= HOUR_SECS;
            deeds.push(Deed::TogetherHour);
        }

        // A smile counts only from a face we can see and trust.
        let smiling = s.face
            && s.expression
                .is_some_and(|e| e.confidence >= SMILE_CONFIDENCE && e.smile > SMILE_LEVEL);
        if smiling {
            self.smile_held += dt;
            if self.smile_held >= SMILE_HOLD_SECS && !self.smile_counted {
                // Counted or not, this smile is spent: a long smile that
                // outlasts the rate limit doesn't count twice.
                self.smile_counted = true;
                if since(book, Deed::Smile, s.now) >= SMILE_EVERY_SECS {
                    deeds.push(Deed::Smile);
                }
            }
        } else {
            self.smile_held = 0.0;
            self.smile_counted = false;
        }

        if LATE_HOURS.contains(&s.hour) && since(book, Deed::LateNight, s.now) >= ONCE_A_NIGHT_SECS
        {
            deeds.push(Deed::LateNight);
        }
    }
}

/// Seconds since `deed` last happened; forever if it never has.
fn since(book: &Remembrance, deed: Deed, now: u64) -> u64 {
    match book.ledger().get(deed).last {
        Some(last) => now.saturating_sub(last),
        None => u64::MAX,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Tuesday, about noon UTC. The hour in `Signals` is set by hand.
    const T0: u64 = 1_790_000_000;

    /// Plays out `secs` seconds, one frame a second, all with the same
    /// face/idle/hour/expression. Returns the firsts it produced.
    struct Sim {
        watch: Watch,
        book: Remembrance,
        now: u64,
        hour: u32,
    }

    impl Sim {
        fn new() -> Sim {
            Sim {
                watch: Watch::new(),
                book: Remembrance::fresh(T0),
                now: T0,
                hour: 12,
            }
        }

        fn run(
            &mut self,
            secs: u64,
            face: bool,
            idle: Option<f64>,
            smile: Option<f32>,
        ) -> Vec<Entry> {
            let mut firsts = Vec::new();
            for _ in 0..secs {
                let s = Signals {
                    now: self.now,
                    dt: 1.0,
                    face,
                    idle_secs: idle,
                    hour: self.hour,
                    expression: smile.map(|smile| Expression {
                        smile,
                        confidence: 0.9,
                        ..Expression::default()
                    }),
                };
                firsts.extend(self.watch.update(&s, &mut self.book));
                self.now += 1;
            }
            firsts
        }

        fn here(&mut self, secs: u64) -> Vec<Entry> {
            self.run(secs, true, None, None)
        }

        fn away(&mut self, secs: u64) -> Vec<Entry> {
            self.run(secs, false, None, None)
        }

        /// The app closed for `secs`: no frames at all.
        fn closed(&mut self, secs: u64) {
            self.now += secs;
        }

        fn count(&self, deed: Deed) -> u64 {
            self.book.ledger().get(deed).count
        }
    }

    #[test]
    fn a_long_stretch_then_a_walk_is_a_break() {
        let mut sim = Sim::new();
        assert!(sim.here(50 * 60).is_empty(), "no firsts in under an hour");
        assert!(sim.away(6 * 60).is_empty());
        assert_eq!(sim.count(Deed::Break), 0, "counted on coming back");
        let firsts = sim.here(1);
        assert_eq!(sim.count(Deed::Break), 1);
        assert_eq!(sim.count(Deed::Return), 1);
        let deeds: Vec<Deed> = firsts.iter().map(|e| e.deed).collect();
        assert_eq!(
            deeds,
            vec![Deed::Break, Deed::Return],
            "break whispered first"
        );

        // The stretch started over: another walk right away isn't a break.
        sim.here(10 * 60);
        sim.away(6 * 60);
        sim.here(1);
        assert_eq!(sim.count(Deed::Break), 1);
    }

    #[test]
    fn a_short_stretch_or_a_short_walk_is_not_a_break() {
        let mut sim = Sim::new();
        sim.here(30 * 60);
        sim.away(6 * 60);
        sim.here(1);
        assert_eq!(sim.count(Deed::Break), 0, "only 30 minutes in");

        let mut sim = Sim::new();
        sim.here(50 * 60);
        sim.away(3 * 60);
        sim.here(1);
        assert_eq!(sim.count(Deed::Break), 0, "3 minutes isn't a break");
        assert_eq!(sim.count(Deed::Return), 1, "but it is a return");
        // ...and the stretch carries on: 50 + 3 min isn't reset.
        sim.away(6 * 60);
        sim.here(1);
        assert_eq!(sim.count(Deed::Break), 1);
    }

    #[test]
    fn a_flickering_face_is_not_coming_and_going() {
        let mut sim = Sim::new();
        for _ in 0..100 {
            sim.here(5);
            sim.away(2);
        }
        assert_eq!(sim.count(Deed::Return), 0);
    }

    #[test]
    fn input_alone_means_here_with_the_camera_off() {
        let mut sim = Sim::new();
        sim.run(50 * 60, false, Some(3.0), None);
        sim.run(6 * 60, false, Some(120.0), None);
        sim.run(1, false, Some(0.5), None);
        assert_eq!(sim.count(Deed::Break), 1);
        // Nothing to go on at all (no camera, no idle time): not here.
        let mut sim = Sim::new();
        sim.run(60, false, None, None);
        assert_eq!(sim.book.last_seen(), None);
    }

    #[test]
    fn a_night_away_is_rest_and_a_morning() {
        let mut sim = Sim::new();
        sim.hour = 23;
        sim.here(60);
        sim.closed(8 * 3600);
        sim.hour = 7;
        let firsts = sim.here(1);
        assert_eq!(sim.count(Deed::Rest), 1);
        assert_eq!(sim.count(Deed::EarlyMorning), 1);
        assert_eq!(sim.count(Deed::Break), 0, "rest, not a break");
        assert_eq!(firsts.first().map(|e| e.deed), Some(Deed::Rest));

        // A nap in the afternoon of the same day isn't a second night.
        sim.hour = 14;
        sim.closed(7 * 3600);
        sim.here(1);
        assert_eq!(sim.count(Deed::Rest), 1);
    }

    #[test]
    fn days_away_is_a_long_return() {
        let mut sim = Sim::new();
        sim.here(60);
        sim.closed(5 * 24 * 3600);
        let firsts = sim.here(1);
        assert_eq!(firsts.first().map(|e| e.deed), Some(Deed::LongReturn));
        assert_eq!(sim.count(Deed::Return), 1);
    }

    #[test]
    fn hours_together_are_counted_whole() {
        let mut sim = Sim::new();
        sim.here(2 * 3600 + 1800);
        assert_eq!(sim.count(Deed::TogetherHour), 2);
    }

    #[test]
    fn a_sleeping_computer_is_not_time_together() {
        let mut sim = Sim::new();
        sim.here(60);
        // One frame claiming 8 hours passed, face in view on both sides.
        let s = Signals {
            now: sim.now + 8 * 3600,
            dt: 8.0 * 3600.0,
            face: true,
            hour: 12,
            ..Signals::default()
        };
        sim.watch.update(&s, &mut sim.book);
        assert_eq!(sim.count(Deed::TogetherHour), 0);
        assert_eq!(sim.count(Deed::Rest), 1, "the clock says it was rest");
    }

    #[test]
    fn a_held_smile_counts_once() {
        let mut sim = Sim::new();
        let flash = Signals {
            now: T0,
            dt: 0.5,
            face: true,
            hour: 12,
            expression: Some(Expression {
                smile: 0.9,
                confidence: 0.9,
                ..Expression::default()
            }),
            ..Signals::default()
        };
        sim.watch.update(&flash, &mut sim.book);
        assert_eq!(sim.count(Deed::Smile), 0, "half a second isn't held");
        let firsts = sim.run(30, true, None, Some(0.9));
        assert_eq!(sim.count(Deed::Smile), 1);
        assert_eq!(firsts[0].deed, Deed::Smile);
        // Another smile soon after is lovely but not counted again...
        sim.run(5, true, None, Some(0.1));
        sim.run(5, true, None, Some(0.9));
        assert_eq!(sim.count(Deed::Smile), 1);
        // ...until a while has passed.
        sim.run(SMILE_EVERY_SECS, true, None, Some(0.1));
        sim.run(5, true, None, Some(0.9));
        assert_eq!(sim.count(Deed::Smile), 2);
    }

    #[test]
    fn an_unsure_smile_does_not_count() {
        let mut sim = Sim::new();
        let s = Signals {
            now: T0,
            dt: 1.0,
            face: true,
            hour: 12,
            expression: Some(Expression {
                smile: 1.0,
                confidence: 0.1,
                ..Expression::default()
            }),
            ..Signals::default()
        };
        for _ in 0..10 {
            sim.watch.update(&s, &mut sim.book);
        }
        assert_eq!(sim.count(Deed::Smile), 0);
    }

    #[test]
    fn a_late_night_counts_once_a_night() {
        let mut sim = Sim::new();
        sim.hour = 2;
        let firsts = sim.here(2 * 3600);
        assert_eq!(sim.count(Deed::LateNight), 1);
        assert!(firsts.iter().any(|e| e.deed == Deed::LateNight));
        sim.hour = 12;
        sim.here(60);
        sim.closed(12 * 3600);
        sim.hour = 2;
        sim.here(60);
        assert_eq!(sim.count(Deed::LateNight), 2, "the next night");
    }
}
