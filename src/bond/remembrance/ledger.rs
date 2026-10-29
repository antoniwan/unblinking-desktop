//! The ledger: one tally per kind of deed, each with a lifetime count and
//! a "recent" sum that fades with time. This is the only part of the
//! memory that is saved; everything else is worked out from it.

use serde::{Deserialize, Serialize};

/// How fast "recent" fades: after this long a deed counts half as much.
/// Three days means a good weekend still glows on Monday, a rough week
/// shows by Thursday, and a deed from a week ago counts about a fifth.
/// Shorter (one day) made the blinki moody overnight; longer (two weeks)
/// made it slow to notice you were back to looking after yourself.
const HALF_LIFE_SECS: f64 = 3.0 * 24.0 * 3600.0;

/// Something good (or just true) that happened, as the blinki saw it.
/// The `Watch` in wellbeing.rs turns what the senses report into these.
///
/// `Serialize`/`Deserialize` with `rename_all = "snake_case"` writes them
/// into the JSON as "break", "long_return" and so on. `Hash`, `Eq` and
/// `Ord` aren't needed: deeds are looked up by their position instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Deed {
    /// Stepped away for a few minutes after a long stretch at the screen.
    Break,
    /// Came back after being away.
    Return,
    /// Came back after days away. Always also a `Return`.
    LongReturn,
    /// Each full hour spent here together.
    TogetherHour,
    /// A real smile, held for a moment.
    Smile,
    /// A long gap between sessions: a night's sleep, most likely.
    Rest,
    /// First here in the early morning, after resting.
    EarlyMorning,
    /// Still here in the small hours. Not a bad deed: it just makes the
    /// blinki a little less calm, the way a late night does to anyone.
    LateNight,
}

impl Deed {
    /// How many kinds there are. `const` means it's known while compiling,
    /// so it can size an array: `[Tally; Deed::COUNT]`.
    pub const COUNT: usize = 8;

    /// Every deed, in the same order as the enum, so `ALL[d as usize] == d`.
    pub const ALL: [Deed; Deed::COUNT] = [
        Deed::Break,
        Deed::Return,
        Deed::LongReturn,
        Deed::TogetherHour,
        Deed::Smile,
        Deed::Rest,
        Deed::EarlyMorning,
        Deed::LateNight,
    ];

    /// The deed's name in the JSON file.
    pub fn key(self) -> &'static str {
        match self {
            Deed::Break => "break",
            Deed::Return => "return",
            Deed::LongReturn => "long_return",
            Deed::TogetherHour => "together_hour",
            Deed::Smile => "smile",
            Deed::Rest => "rest",
            Deed::EarlyMorning => "early_morning",
            Deed::LateNight => "late_night",
        }
    }

    /// What the blinki whispers the first time it sees this deed. In the
    /// same voice as the mind's own lines: short, a little eerie, fond underneath.
    pub fn first_line(self) -> &'static str {
        match self {
            Deed::Break => "You rested your eyes. Good. I kept watch.",
            Deed::Return => "You came back. I will remember that.",
            Deed::LongReturn => "It has been long. I did not forget you.",
            Deed::TogetherHour => "A whole hour, together.",
            Deed::Smile => "You smiled at me. I will keep it.",
            Deed::Rest => "You slept. The fire slept a little too.",
            Deed::EarlyMorning => "Up with the light. I saw.",
            Deed::LateNight => "The small hours. Just us.",
        }
    }
}

/// Everything remembered about one kind of deed.
///
/// `recent` is stored as "its value at time `at`": fading is worked out
/// when it's read (see `recent_at`), so nothing has to tick in the
/// background, and a blinki left closed for a month fades correctly the
/// moment it opens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)] // a missing field takes its `Default` value
pub struct Tally {
    /// How many times, ever. Only grows.
    pub count: u64,
    /// The fading sum, as it stood at `at`.
    pub recent: f64,
    /// When `recent` was last brought up to date, unix seconds.
    pub at: u64,
    /// The first and latest time it happened.
    pub first: Option<u64>,
    pub last: Option<u64>,
}

impl Tally {
    /// The fading sum as of `now`: halved for every `HALF_LIFE_SECS` since
    /// `at`. That's exponential decay, `value * 0.5^(elapsed / half_life)`.
    /// A clock that went backwards counts as no time passing.
    pub fn recent_at(&self, now: u64) -> f64 {
        decay(self.recent, now.saturating_sub(self.at))
    }

    fn add(&mut self, now: u64) {
        self.recent = self.recent_at(now) + 1.0;
        self.at = self.at.max(now);
        self.count += 1;
        self.first.get_or_insert(now);
        self.last = Some(now);
    }
}

/// `value` after `elapsed` seconds of fading.
fn decay(value: f64, elapsed: u64) -> f64 {
    value * 0.5f64.powf(elapsed as f64 / HALF_LIFE_SECS)
}

/// One tally per kind of deed. An array indexed by `deed as usize` (an
/// enum without data turns into its position: Break is 0, Return is 1...).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ledger {
    tallies: [Tally; Deed::COUNT],
}

impl Ledger {
    pub fn get(&self, deed: Deed) -> &Tally {
        &self.tallies[deed as usize]
    }

    pub fn add(&mut self, deed: Deed, now: u64) {
        self.tallies[deed as usize].add(now);
    }

    /// Puts back a tally read from the file. `pub(super)` makes it visible
    /// to the parent module (remembrance, which reads the file) and the
    /// modules inside it, but not to the rest of the app.
    pub(super) fn set(&mut self, deed: Deed, tally: Tally) {
        self.tallies[deed as usize] = tally;
    }

    pub(super) fn recent(&self, deed: Deed, now: u64) -> f64 {
        self.get(deed).recent_at(now)
    }

    pub(super) fn count(&self, deed: Deed) -> f64 {
        self.get(deed).count as f64
    }

    /// Lifetime points: every deed ever, weighted like `bond` likes them.
    /// Late nights count too; they are still time spent together.
    pub(super) fn lifetime_points(&self) -> f64 {
        self.count(Deed::TogetherHour)
            + self.count(Deed::Break)
            + self.count(Deed::Rest)
            + 0.5 * self.count(Deed::Smile)
            + 0.5 * self.count(Deed::Return)
            + 2.0 * self.count(Deed::LongReturn)
            + 0.5 * self.count(Deed::EarlyMorning)
            + 0.5 * self.count(Deed::LateNight)
    }

    /// The good things, recently: what feeds the day's brightness.
    pub(super) fn recent_care(&self, now: u64) -> f64 {
        let r = |d| self.recent(d, now);
        r(Deed::Break)
            + r(Deed::Rest)
            + 0.5 * r(Deed::Return)
            + 0.5 * r(Deed::Smile)
            + 0.3 * r(Deed::TogetherHour)
            + 0.5 * r(Deed::EarlyMorning)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const DAY: u64 = 24 * 3600;
    /// Some Tuesday in 2026, so times look like real dates.
    const T0: u64 = 1_790_000_000;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn recent_halves_every_half_life() {
        let mut ledger = Ledger::default();
        ledger.add(Deed::Break, T0);
        let t = ledger.get(Deed::Break);
        assert!(close(t.recent_at(T0), 1.0));
        let half = HALF_LIFE_SECS as u64;
        assert!(close(t.recent_at(T0 + half), 0.5));
        assert!(close(t.recent_at(T0 + 2 * half), 0.25));
        // A clock that went backwards is "no time passed", not growth.
        assert!(close(t.recent_at(T0 - DAY), 1.0));
    }

    #[test]
    fn adding_decays_first_then_adds() {
        let mut ledger = Ledger::default();
        let half = HALF_LIFE_SECS as u64;
        ledger.add(Deed::Smile, T0);
        ledger.add(Deed::Smile, T0 + half);
        let t = ledger.get(Deed::Smile);
        assert!(close(t.recent_at(T0 + half), 1.5));
        assert_eq!(t.count, 2);
        assert_eq!(t.first, Some(T0));
        assert_eq!(t.last, Some(T0 + half));
    }

    #[test]
    fn every_deed_has_its_own_key_and_line() {
        for (i, d) in Deed::ALL.iter().enumerate() {
            assert_eq!(*d as usize, i, "ALL is in enum order");
            assert!(!d.first_line().is_empty());
            // The serde name and `key()` must agree, or the file's ledger
            // and journal would disagree about names.
            assert_eq!(serde_json::to_value(d).unwrap(), Value::from(d.key()));
        }
    }
}
