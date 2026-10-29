//! Feeling: which of the eight moods fits the moment, and how the blinki's
//! slow temperament colours the look.
//!
//! Three things feed it, from fast to slow:
//!
//! - what's happening now (a bang, a face, the hour, a crowd): the studio's
//!   triggers, which main.rs used to check by itself;
//! - your face, if you opted in to "Read my face": a held smile delights
//!   it, and looking away for a while makes it wonder, then pine;
//! - its temperament, from what it remembers of your days (bond::remembrance):
//!   a fond blinki lights up sooner and gets lonely slower, a jumpy one
//!   stays rattled after a bang, a tired one gets drowsy earlier.
//!
//! It's glad about what's good for you (you coming back, smiling, resting),
//! never about time spent staring at it.
//!
//! `choose` is a plain function: same inputs, same answer, so it's easy to
//! test. `Feeling` wraps it with a little memory (how long you've smiled,
//! how long the mood has been showing) so the eye doesn't flicker.

use crate::bond::remembrance::Temperament;
use crate::mind::moods::Look;
use crate::mind::{Line, Mode};
use crate::senses::expression::Expression;

/// Everything the choice looks at, gathered by the app (app/frame.rs) each frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Context {
    pub mode: Mode,
    /// The latest whisper's kind, and how many seconds ago it was said.
    pub last_said: Option<(Line, f32)>,
    /// Seconds since a face was last in view (since launch, if never).
    pub no_face_secs: f32,
    /// The local hour, 0 to 23.
    pub hour: u32,
    /// Seconds since it last heard anything louder than quiet.
    pub quiet_secs: f32,
    /// A face far too close to the camera.
    pub too_close: bool,
    /// How many people are in view (`mind::crowd_size`).
    pub crowd: usize,
    /// Its slow temperament, while "Remember our days" is on.
    pub temperament: Option<Temperament>,
    /// The reading of your face, while "Read my face" is on.
    pub expression: Option<Expression>,
    /// How long you've been up and here past midnight tonight, in seconds.
    /// A long late night makes it worry about you.
    pub up_late_secs: f32,
}

/// A mood and why, for Blinki's Senses. `&'static str` is text baked into
/// the program, so a Choice is cheap to copy around.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice {
    pub mood: &'static str,
    pub reason: &'static str,
}

impl Choice {
    const fn new(mood: &'static str, reason: &'static str) -> Choice {
        Choice { mood, reason }
    }
}

/// How long, in seconds, your face has been doing something steadily.
/// `Feeling` keeps these; tests can make them up.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Held {
    /// Smiling, as the face reading sees it.
    pub smiling: f32,
    /// In view but turned away from the screen.
    pub turned_away: f32,
}

/// Looking away this long (while still in view) makes it wonder.
const AWAY_CURIOUS_SECS: f32 = 8.0;
/// Hours before this count as night (the morning end of "late").
const MORNING_HOUR: f32 = 6.0;
/// Up and here past midnight this long, it starts to worry (20 minutes).
const WORRY_SECS: f32 = 20.0 * 60.0;

/// The thresholds a temperament bends. With no temperament (or a neutral
/// one) they're exactly the studio's numbers that main.rs used before.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Leanings {
    /// A bang makes it wrathful for this long.
    wrath_secs: f32,
    /// ...then suspicious for this much longer (0 unless it's jumpy).
    rattled_secs: f32,
    /// You coming back delights it for this long.
    delight_secs: f32,
    /// A smile held this long delights it.
    smile_hold: f32,
    /// No face (or a face turned away) this long makes it lonely.
    lonely_secs: f32,
    /// From this hour on it's night.
    bedtime: f32,
    /// Quiet this long, at night, makes it drowsy.
    quiet_secs: f32,
    /// Up past midnight this long makes it worry about you.
    worry_secs: f32,
}

impl Leanings {
    fn of(t: Option<&Temperament>) -> Leanings {
        // No memory means a neutral temperament, which changes nothing.
        let t = t.copied().unwrap_or_default();
        let warmth = lean(t.warmth);
        let calm = lean(t.calm);
        let vigor = lean(t.vigor);
        let bond = unit(t.bond, 0.0);
        // Fondness: warm and well known. 0 when neutral, up to 1.
        let fond = (warmth + bond) / 2.0;
        Leanings {
            // A calm blinki's anger passes sooner: 1.2 s to 4.8 s.
            wrath_secs: 3.0 * (1.0 - 0.6 * calm),
            // A jumpy one stays wary after: up to 8 s more.
            rattled_secs: 8.0 * (-calm).max(0.0),
            // A fond one is glad longer when you come back: 3 s to 6 s,
            delight_secs: 4.0 * (1.0 + 0.5 * fond),
            // and lights up at a shorter smile: 0.9 s to 1.8 s,
            smile_hold: 1.5 * (1.0 - 0.4 * fond),
            // and misses you less quickly. `powf` keeps this a gentle
            // doubling or halving: 42 s to 120 s.
            lonely_secs: 60.0 * 2f32.powf(fond),
            // A tired one goes to bed earlier: 21:00 to midnight,
            bedtime: (23.0 + 2.0 * vigor).min(24.0),
            // and needs less quiet to doze: 60 s to 240 s.
            quiet_secs: 120.0 * 2f32.powf(vigor),
            // A fond one worries sooner (10 to 20 min), and one already
            // unsettled by late nights a little sooner still.
            worry_secs: WORRY_SECS * 2f32.powf(-fond) * (1.0 + 0.25 * calm),
        }
    }
}

/// A 0..1 trait as -1..1 around neutral. A NaN reads as neutral.
fn lean(x: f32) -> f32 {
    (unit(x, 0.5) - 0.5) * 2.0
}

/// Clamps to 0..1; anything that isn't a number becomes `fallback`.
fn unit(x: f32, fallback: f32) -> f32 {
    if x.is_finite() {
        x.clamp(0.0, 1.0)
    } else {
        fallback
    }
}

/// Picks the mood, most urgent first. With `temperament` and `expression`
/// both None (and nothing held) this is exactly what main.rs's
/// choose_mood did.
pub fn choose(ctx: &Context, held: &Held) -> Choice {
    let l = Leanings::of(ctx.temperament.as_ref());
    // A closure that captures `ctx`: "was one of these said within `secs`?"
    let said = |kinds: &[Line], secs: f32| {
        ctx.last_said
            .is_some_and(|(kind, ago)| kinds.contains(&kind) && ago < secs)
    };
    let hour = ctx.hour as f32;
    let late = hour < MORNING_HOUR || hour >= l.bedtime;
    let quiet = ctx.quiet_secs > l.quiet_secs;
    let present = ctx.mode == Mode::Locked;

    if ctx.mode == Mode::Blind {
        Choice::new("Blinded", "you blinded it")
    } else if said(&[Line::Loud], l.wrath_secs) {
        Choice::new("Wrathful", "a bang, just now")
    } else if ctx.too_close {
        Choice::new("Wrathful", "a face far too close")
    } else if said(&[Line::Returned, Line::First], l.delight_secs) {
        Choice::new("Delighted", "you came into view")
    } else if held.smiling >= l.smile_hold {
        Choice::new("Delighted", "you smiled")
    } else if ctx.crowd > 1 {
        Choice::new("Suspicious", "two or more people")
    } else if said(&[Line::Loud], l.wrath_secs + l.rattled_secs) {
        Choice::new("Suspicious", "still rattled by that bang")
    } else if ctx.up_late_secs >= l.worry_secs {
        Choice::new("Worried", "you're up so late")
    } else if late && quiet {
        Choice::new("Drowsy", "late, and quiet for a while")
    } else if present && held.turned_away >= l.lonely_secs {
        Choice::new("Lonely", "you're here, but turned away")
    } else if present && held.turned_away >= AWAY_CURIOUS_SECS {
        Choice::new("Curious", "you looked away; at what?")
    } else if present {
        Choice::new("Watchful", "a face in view")
    } else if ctx.no_face_secs > l.lonely_secs {
        Choice::new("Lonely", "no face for a while")
    } else {
        Choice::new("Curious", "looking around")
    }
}

/// How worried it is about you being up late, and how it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Worry {
    /// What it might say; one is picked at random.
    pub lines: &'static [&'static str],
    /// It says one at most this often, in seconds.
    pub every_secs: f32,
    /// Extra tremble on top of the Worried look's own shake.
    pub tremble: f32,
}

/// The later it gets, the more it insists: gentle for the first hour past
/// midnight, firmer in the second, plainly pleading after that. It never
/// scolds, and it never stops you; you can always ignore it.
pub fn worry(up_late_secs: f32) -> Worry {
    let minutes = up_late_secs / 60.0;
    if minutes < 60.0 {
        Worry {
            lines: &[
                "It's late. Rest your eyes.",
                "Sleep. I will keep watch.",
                "Even I close my eye sometimes.",
            ],
            every_secs: 30.0 * 60.0,
            tremble: 0.0,
        }
    } else if minutes < 120.0 {
        Worry {
            lines: &[
                "It's very late. Go to bed.",
                "Whatever it is will keep until morning.",
                "Your eyes are tired. I can tell.",
            ],
            every_secs: 20.0 * 60.0,
            tremble: 0.1,
        }
    } else {
        Worry {
            lines: &[
                "Please. Sleep.",
                "The sun will rise soon. Rest before it does.",
                "I will still be here. Go.",
            ],
            every_secs: 15.0 * 60.0,
            tremble: 0.2,
        }
    }
}

/// A face reading counts only if the reader is at least this sure.
const MIN_CONFIDENCE: f32 = 0.5;
/// Smiling starts above this and stops below the lower one. Two thresholds
/// (hysteresis) stop a smile hovering at one value from flickering on/off.
const SMILE_ON: f32 = 0.6;
const SMILE_OFF: f32 = 0.4;
/// Turned away below this `facing`, back again above the higher one.
const AWAY_ON: f32 = 0.35;
const AWAY_OFF: f32 = 0.5;
/// A lapse shorter than this (a blink, a missed frame) doesn't reset a held smile.
const GRACE_SECS: f32 = 0.4;
/// A calm mood shows at least this long before another calm one replaces it.
const MIN_DWELL_SECS: f32 = 2.5;
/// ...and the new one has to be wanted this long without a break.
const STEADY_SECS: f32 = 0.5;
/// These answer something that just happened, so they show at once.
const URGENT: [&str; 3] = ["Blinded", "Wrathful", "Delighted"];

/// `choose` plus memory: how long your face has held a smile or looked
/// away, and how long the shown mood has been up, so moods don't flicker.
#[derive(Clone, Debug, Default)]
pub struct Feeling {
    held: Held,
    smiling: bool,
    away: bool,
    /// Seconds since the smile or the turning away last lapsed.
    smile_lapse: f32,
    away_lapse: f32,
    shown: Option<Choice>,
    shown_for: f32,
    /// A different mood `choose` keeps asking for, and for how long.
    pending: Option<&'static str>,
    pending_for: f32,
}

impl Feeling {
    pub fn new() -> Feeling {
        Feeling::default()
    }

    /// What it's been holding, for Blinki's Senses.
    #[allow(dead_code)] // until Blinki's Senses shows it
    pub fn held(&self) -> Held {
        self.held
    }

    /// Moves time on by `dt` seconds and returns the mood to show.
    pub fn update(&mut self, dt: f32, ctx: &Context) -> Choice {
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.notice(dt, ctx.expression);
        let want = choose(ctx, &self.held);
        self.settle(dt, want)
    }

    /// Keeps the face's held times up to date.
    fn notice(&mut self, dt: f32, expression: Option<Expression>) {
        // `filter` keeps the Some only if the test passes.
        match expression.filter(|e| e.confidence >= MIN_CONFIDENCE) {
            Some(e) => {
                self.smiling = e.smile >= if self.smiling { SMILE_OFF } else { SMILE_ON };
                self.away = e.facing < if self.away { AWAY_OFF } else { AWAY_ON };
            }
            None => {
                self.smiling = false;
                self.away = false;
            }
        }
        tally(
            &mut self.held.smiling,
            &mut self.smile_lapse,
            self.smiling,
            dt,
        );
        tally(
            &mut self.held.turned_away,
            &mut self.away_lapse,
            self.away,
            dt,
        );
    }

    /// Decides whether `want` replaces the mood being shown.
    fn settle(&mut self, dt: f32, want: Choice) -> Choice {
        // `let ... else`: the first frame shows whatever is wanted.
        let Some(shown) = self.shown else {
            self.shown = Some(want);
            return want;
        };
        self.shown_for += dt;
        if want.mood == shown.mood {
            // Same mood, maybe a fresher reason.
            self.shown = Some(want);
            self.pending = None;
            return want;
        }
        if self.pending == Some(want.mood) {
            self.pending_for += dt;
        } else {
            self.pending = Some(want.mood);
            self.pending_for = 0.0;
        }
        // Urgent moods break in at once, and unblinding ends Blinded at once.
        let urgent = URGENT.contains(&want.mood) || shown.mood == "Blinded";
        let settled = self.shown_for >= MIN_DWELL_SECS && self.pending_for >= STEADY_SECS;
        if urgent || settled {
            self.shown = Some(want);
            self.shown_for = 0.0;
            self.pending = None;
            want
        } else {
            shown
        }
    }
}

/// Adds up `held` while `active`; a lapse longer than the grace starts it over.
/// `&mut f32` lets the function change the caller's numbers in place.
fn tally(held: &mut f32, lapse: &mut f32, active: bool, dt: f32) {
    if active {
        *held += dt;
        *lapse = 0.0;
    } else {
        *lapse += dt;
        if *lapse > GRACE_SECS {
            *held = 0.0;
        }
    }
}

/// Lets the temperament, and how today has gone, colour the mood's look a
/// little. `day_brightness` is 0..1 with 0.5 an ordinary day: lower is a day
/// without breaks or rest, higher a good one. Never dramatic: no number moves
/// by more than about a fifth, and a neutral temperament on an ordinary day
/// changes nothing.
///
/// - warmth: a fond blinki glows and burns a touch hotter, a bit redder;
/// - vigor: a lively one's flames drift faster and throw more embers;
/// - calm: a settled one shakes less, a jumpy one more;
/// - a dull, neglected day dims it slightly; a good one brightens the glow.
pub fn tint(look: Look, t: &Temperament, day_brightness: f32) -> Look {
    let warmth = lean(t.warmth);
    let calm = lean(t.calm);
    let vigor = lean(t.vigor);
    let bond = unit(t.bond, 0.0);
    let day = lean(day_brightness);

    // `look` was passed by value (Look is Copy), so we can change our copy.
    let mut out = look;
    out.glow *= 1.0 + 0.08 * warmth + 0.04 * bond + 0.05 * day;
    out.heat *= 1.0 + 0.06 * warmth;
    out.flow *= 1.0 + 0.12 * vigor;
    out.embers *= 1.0 + 0.15 * vigor;
    out.wake *= 1.0 + 0.04 * vigor;
    out.shake *= 1.0 - 0.2 * calm;
    // A neglected day: at most a tenth dimmer. Never brightens past the mood.
    out.dim = out.dim.max(0.1 * (-day).max(0.0));
    for c in &mut out.colors {
        c[0] = (c[0] * (1.0 + 0.04 * warmth)).min(1.0);
        c[2] *= 1.0 - 0.04 * warmth;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mind::moods::Moods;

    /// A quiet afternoon with nobody around and nothing said.
    fn calm_day() -> Context {
        Context {
            mode: Mode::Hunting,
            last_said: None,
            no_face_secs: 0.0,
            hour: 14,
            quiet_secs: 0.0,
            too_close: false,
            crowd: 0,
            temperament: None,
            expression: None,
            up_late_secs: 0.0,
        }
    }

    fn mood(ctx: &Context) -> &'static str {
        choose(ctx, &Held::default()).mood
    }

    fn temper(warmth: f32, calm: f32, vigor: f32, bond: f32) -> Temperament {
        Temperament {
            warmth,
            calm,
            vigor,
            bond,
        }
    }

    fn face(smile: f32, facing: f32) -> Expression {
        Expression {
            smile,
            surprise: 0.0,
            facing,
            confidence: 0.9,
        }
    }

    /// main.rs's choose_mood as it was, word for word in logic, to check
    /// the new one against.
    fn old_choose(ctx: &Context) -> &'static str {
        let said = |kinds: &[Line], secs: f32| {
            ctx.last_said
                .is_some_and(|(k, ago)| kinds.contains(&k) && ago < secs)
        };
        let late = !(6..23).contains(&ctx.hour);
        let quiet = ctx.quiet_secs > 120.0;
        if ctx.mode == Mode::Blind {
            "Blinded"
        } else if said(&[Line::Loud], 3.0) || ctx.too_close {
            "Wrathful"
        } else if said(&[Line::Returned, Line::First], 4.0) {
            "Delighted"
        } else if ctx.crowd > 1 {
            "Suspicious"
        } else if late && quiet {
            "Drowsy"
        } else if ctx.mode == Mode::Locked {
            "Watchful"
        } else if ctx.no_face_secs > 60.0 {
            "Lonely"
        } else {
            "Curious"
        }
    }

    /// Lots of moments, mixing every input the old code read.
    fn many_moments() -> Vec<Context> {
        let mut out = Vec::new();
        let saids = [
            None,
            Some((Line::Loud, 1.0)),
            Some((Line::Loud, 3.5)),
            Some((Line::Returned, 2.0)),
            Some((Line::First, 5.0)),
            Some((Line::Gone, 0.5)),
        ];
        for mode in [Mode::Locked, Mode::Hunting, Mode::Blind] {
            for last_said in saids {
                for hour in [0, 5, 6, 14, 22, 23] {
                    for quiet_secs in [10.0, 130.0] {
                        for no_face_secs in [5.0, 61.0] {
                            for too_close in [false, true] {
                                for crowd in [0, 1, 2] {
                                    out.push(Context {
                                        mode,
                                        last_said,
                                        no_face_secs,
                                        hour,
                                        quiet_secs,
                                        too_close,
                                        crowd,
                                        temperament: None,
                                        expression: None,
                                        up_late_secs: 0.0,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        out
    }

    #[test]
    fn without_memory_or_face_it_chooses_as_before() {
        for ctx in many_moments() {
            assert_eq!(mood(&ctx), old_choose(&ctx), "{ctx:?}");
        }
    }

    #[test]
    fn a_neutral_temperament_changes_nothing() {
        for mut ctx in many_moments() {
            ctx.temperament = Some(Temperament::default());
            assert_eq!(mood(&ctx), old_choose(&ctx), "{ctx:?}");
        }
        assert_eq!(
            Leanings::of(Some(&Temperament::default())),
            Leanings::of(None)
        );
    }

    #[test]
    fn the_old_priorities_hold() {
        let mut ctx = calm_day();
        assert_eq!(mood(&ctx), "Curious");
        ctx.no_face_secs = 61.0;
        assert_eq!(mood(&ctx), "Lonely");
        ctx.mode = Mode::Locked;
        assert_eq!(mood(&ctx), "Watchful");
        ctx.crowd = 2;
        assert_eq!(mood(&ctx), "Suspicious");
        ctx.last_said = Some((Line::Returned, 1.0));
        assert_eq!(mood(&ctx), "Delighted");
        ctx.too_close = true;
        assert_eq!(mood(&ctx), "Wrathful");
        ctx.mode = Mode::Blind;
        assert_eq!(mood(&ctx), "Blinded");
    }

    #[test]
    fn a_held_smile_delights_it() {
        let ctx = Context {
            mode: Mode::Locked,
            ..calm_day()
        };
        let brief = Held {
            smiling: 1.0,
            ..Held::default()
        };
        assert_eq!(choose(&ctx, &brief).mood, "Watchful");
        let held = Held {
            smiling: 1.6,
            ..Held::default()
        };
        assert_eq!(choose(&ctx, &held), Choice::new("Delighted", "you smiled"));
        // Even with company: a shared laugh beats suspicion.
        let crowd = Context { crowd: 2, ..ctx };
        assert_eq!(choose(&crowd, &held).mood, "Delighted");
        // But never over a bang.
        let bang = Context {
            last_said: Some((Line::Loud, 0.5)),
            ..ctx
        };
        assert_eq!(choose(&bang, &held).mood, "Wrathful");
    }

    #[test]
    fn looking_away_makes_it_wonder_then_pine() {
        let ctx = Context {
            mode: Mode::Locked,
            ..calm_day()
        };
        let away = |secs| Held {
            turned_away: secs,
            ..Held::default()
        };
        assert_eq!(choose(&ctx, &away(3.0)).mood, "Watchful");
        assert_eq!(choose(&ctx, &away(10.0)).mood, "Curious");
        assert_eq!(choose(&ctx, &away(70.0)).mood, "Lonely");
    }

    #[test]
    fn a_fond_blinki_lights_up_sooner_and_misses_you_slower() {
        let fond = temper(0.9, 0.5, 0.5, 0.8);
        let locked = Context {
            mode: Mode::Locked,
            ..calm_day()
        };
        let smile = Held {
            smiling: 1.1,
            ..Held::default()
        };
        assert_eq!(choose(&locked, &smile).mood, "Watchful", "neutral waits");
        let warm = Context {
            temperament: Some(fond),
            ..locked
        };
        assert_eq!(choose(&warm, &smile).mood, "Delighted", "fond lights up");

        let gone = Context {
            no_face_secs: 90.0,
            ..calm_day()
        };
        assert_eq!(mood(&gone), "Lonely");
        let gone_fond = Context {
            temperament: Some(fond),
            ..gone
        };
        assert_eq!(mood(&gone_fond), "Curious", "it trusts you'll be back");

        let back = Context {
            last_said: Some((Line::Returned, 5.0)),
            ..locked
        };
        assert_eq!(mood(&back), "Watchful");
        let back_fond = Context {
            temperament: Some(fond),
            ..back
        };
        assert_eq!(mood(&back_fond), "Delighted", "glad for longer");
    }

    #[test]
    fn a_cold_blinki_gets_lonely_sooner() {
        let ctx = Context {
            no_face_secs: 50.0,
            temperament: Some(temper(0.0, 0.5, 0.5, 0.0)),
            ..calm_day()
        };
        assert_eq!(mood(&ctx), "Lonely");
    }

    #[test]
    fn calm_shapes_how_long_a_bang_lingers() {
        let bang = |ago, calm| Context {
            mode: Mode::Locked,
            last_said: Some((Line::Loud, ago)),
            temperament: Some(temper(0.5, calm, 0.5, 0.0)),
            ..calm_day()
        };
        // Neutral: 3 s of wrath, then straight back.
        assert_eq!(mood(&bang(2.0, 0.5)), "Wrathful");
        assert_eq!(mood(&bang(4.0, 0.5)), "Watchful");
        // Jumpy: wrath for longer, then still wary.
        assert_eq!(mood(&bang(4.0, 0.1)), "Wrathful");
        assert_eq!(mood(&bang(7.0, 0.1)), "Suspicious");
        assert_eq!(mood(&bang(20.0, 0.1)), "Watchful");
        // Settled: the anger passes quickly.
        assert_eq!(mood(&bang(2.0, 0.95)), "Watchful");
    }

    #[test]
    fn a_tired_blinki_dozes_earlier() {
        let evening = Context {
            hour: 22,
            quiet_secs: 70.0,
            ..calm_day()
        };
        assert_eq!(mood(&evening), "Curious");
        let tired = Context {
            temperament: Some(temper(0.5, 0.5, 0.05, 0.0)),
            ..evening
        };
        assert_eq!(mood(&tired), "Drowsy");
        // A lively one stays up past 23:00.
        let lively = Context {
            hour: 23,
            quiet_secs: 200.0,
            temperament: Some(temper(0.5, 0.5, 1.0, 0.0)),
            ..calm_day()
        };
        assert_eq!(mood(&lively), "Curious");
    }

    #[test]
    fn junk_numbers_read_as_neutral() {
        let t = temper(f32::NAN, f32::INFINITY, -3.0, f32::NAN);
        let l = Leanings::of(Some(&t));
        assert!(l.wrath_secs.is_finite() && l.lonely_secs.is_finite());
        assert_eq!(l.delight_secs, 4.0, "NaN warmth and bond are neutral");
    }

    /// Runs `secs` of frames at 60 fps through a Feeling.
    fn run(f: &mut Feeling, ctx: &Context, secs: f32) -> Choice {
        let mut last = f.update(0.0, ctx);
        for _ in 0..(secs * 60.0) as usize {
            last = f.update(1.0 / 60.0, ctx);
        }
        last
    }

    #[test]
    fn a_smile_has_to_be_held_and_survives_a_blink() {
        let mut f = Feeling::new();
        let smiling = Context {
            mode: Mode::Locked,
            expression: Some(face(0.8, 1.0)),
            ..calm_day()
        };
        assert_eq!(run(&mut f, &smiling, 1.0).mood, "Watchful");
        // One frame where the reader loses track doesn't start it over.
        f.update(
            1.0 / 60.0,
            &Context {
                expression: None,
                ..smiling
            },
        );
        assert_eq!(run(&mut f, &smiling, 0.6).mood, "Delighted");
        // A smile that fades to 0.5 still counts (hysteresis)...
        let fading = Context {
            expression: Some(face(0.5, 1.0)),
            ..smiling
        };
        assert_eq!(run(&mut f, &fading, 1.0).mood, "Delighted");
        // ...but a straight face ends it, once the mood has had its moment.
        let straight = Context {
            expression: Some(face(0.1, 1.0)),
            ..smiling
        };
        assert_eq!(run(&mut f, &straight, 4.0).mood, "Watchful");
    }

    #[test]
    fn an_unsure_reading_is_ignored() {
        let mut f = Feeling::new();
        let ctx = Context {
            mode: Mode::Locked,
            expression: Some(Expression {
                confidence: 0.2,
                ..face(1.0, 0.0)
            }),
            ..calm_day()
        };
        assert_eq!(run(&mut f, &ctx, 20.0).mood, "Watchful");
        assert_eq!(f.held(), Held::default());
    }

    #[test]
    fn calm_moods_dont_flicker() {
        let mut f = Feeling::new();
        let watching = Context {
            mode: Mode::Locked,
            ..calm_day()
        };
        let looking = calm_day();
        assert_eq!(run(&mut f, &watching, 0.5).mood, "Watchful");
        // The face drops out for a few frames, back and forth.
        for _ in 0..10 {
            assert_eq!(run(&mut f, &looking, 0.2).mood, "Watchful");
            run(&mut f, &watching, 0.1);
        }
        // Gone for real: it switches once the dwell and steadiness pass.
        assert_eq!(run(&mut f, &looking, 3.0).mood, "Curious");
    }

    #[test]
    fn urgent_moods_break_in_at_once() {
        let mut f = Feeling::new();
        let watching = Context {
            mode: Mode::Locked,
            ..calm_day()
        };
        run(&mut f, &watching, 0.1);
        let bang = Context {
            last_said: Some((Line::Loud, 0.0)),
            ..watching
        };
        assert_eq!(f.update(1.0 / 60.0, &bang).mood, "Wrathful");
        let blind = Context {
            mode: Mode::Blind,
            ..watching
        };
        assert_eq!(f.update(1.0 / 60.0, &blind).mood, "Blinded");
        // And unblinding ends Blinded straight away too.
        assert_eq!(f.update(1.0 / 60.0, &watching).mood, "Watchful");
    }

    #[test]
    fn a_neutral_tint_is_no_tint() {
        let look = Moods::load().now();
        assert_eq!(tint(look, &Temperament::default(), 0.5), look);
    }

    #[test]
    fn tints_stay_subtle() {
        let look = Moods::load().now();
        for t in [temper(0.0, 0.0, 0.0, 0.0), temper(1.0, 1.0, 1.0, 1.0)] {
            for day in [0.0, 1.0] {
                let out = tint(look, &t, day);
                for (a, b) in [
                    (look.glow, out.glow),
                    (look.heat, out.heat),
                    (look.flow, out.flow),
                    (look.embers, out.embers),
                    (look.wake, out.wake),
                    (look.swirl, out.swirl),
                ] {
                    assert!((b / a - 1.0).abs() <= 0.2, "{a} to {b}");
                }
                assert!(out.dim <= 0.1);
            }
        }
    }

    #[test]
    fn warmth_glows_vigor_flows_neglect_dims() {
        let look = Moods::load().now();
        let warm = tint(look, &temper(1.0, 0.5, 0.5, 0.0), 0.5);
        assert!(warm.glow > look.glow && warm.heat > look.heat);
        let lively = tint(look, &temper(0.5, 0.5, 1.0, 0.0), 0.5);
        assert!(lively.flow > look.flow && lively.embers > look.embers);
        let neglected = tint(look, &Temperament::default(), 0.0);
        assert!(neglected.dim > look.dim && neglected.glow < look.glow);
        let junk = tint(look, &temper(f32::NAN, 0.5, 0.5, 0.0), f32::NAN);
        assert_eq!(junk, look, "junk in, no change out");
    }

    #[test]
    fn a_long_late_night_makes_it_worry() {
        let night = Context {
            mode: Mode::Locked,
            hour: 2,
            ..calm_day()
        };
        assert_eq!(mood(&night), "Watchful", "a little past midnight is fine");
        let long = Context {
            up_late_secs: 21.0 * 60.0,
            ..night
        };
        assert_eq!(mood(&long), "Worried");
        // A fond blinki worries sooner.
        let fond = Context {
            up_late_secs: 12.0 * 60.0,
            temperament: Some(temper(1.0, 0.5, 0.5, 1.0)),
            ..night
        };
        assert_eq!(mood(&fond), "Worried");
        assert_eq!(
            mood(&Context {
                temperament: None,
                ..fond
            }),
            "Watchful"
        );
        // A bang still wins over worry.
        let bang = Context {
            last_said: Some((Line::Loud, 0.5)),
            ..long
        };
        assert_eq!(mood(&bang), "Wrathful");
    }

    #[test]
    fn worry_grows_more_insistent() {
        let early = worry(30.0 * 60.0);
        let later = worry(90.0 * 60.0);
        let latest = worry(3.0 * 3600.0);
        assert!(early.every_secs > later.every_secs && later.every_secs > latest.every_secs);
        assert!(early.tremble < later.tremble && later.tremble < latest.tremble);
        assert_ne!(early.lines, latest.lines);
    }
}
