//! Remembrance: what the blinki remembers of your days together, after
//! Lifesworn's Remembrance. Good deeds (breaks, rest, coming back, smiling)
//! go into a ledger; its temperament is worked out from that ledger.
//! Everything stays on this computer.
//!
//! The idea, borrowed from Lifesworn: the blinki is fed by how well *you*
//! are doing, not by how long you stare at the screen. Taking breaks,
//! sleeping, coming back, smiling and simply being here together are what
//! it remembers. If you leave it alone for a while its days get dimmer and
//! quieter. It never dies, never sulks, never makes you feel guilty.
//!
//! A few rules this file keeps:
//!
//! - **The ledger is the truth.** Only deeds are saved: how many of each
//!   (ever) and a "recent" sum that fades with time. The temperament is a
//!   pure formula of the ledger, worked out fresh every time it's asked
//!   for, so changing the formula (and bumping `FORMULA_VERSION`) changes
//!   every blinki at once, with nothing to migrate.
//! - **No currency, no numbers on screen.** Nothing here is spent, bought
//!   or reset. The numbers are for the fire's colour and pace, not for you.
//! - **Time comes in as an argument.** Every function that cares about the
//!   clock takes `now` (unix seconds), so tests can play out a week in a
//!   millisecond. `unix_now()` is the one place that reads the real clock.
//! - **A broken file is never a crash.** Like prefs.rs, anything unreadable
//!   is skipped and the rest is kept.

// The app wires this module in a step after this one (main.rs and the
// tray are owned elsewhere), so for now most of this public API is unused
// by the binary and only exercised by the tests. Remove once it's wired.
#![allow(dead_code, reason = "public API not wired into main.rs yet")]

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Bump this whenever `temperament_of` (or `day_brightness_of`) changes
/// meaning. The saved file notes the version it was written under, but the
/// temperament itself is never saved: it's always recomputed from the
/// ledger with the formula in this build. So an old file simply gets the
/// new formula; the number is there so a person reading the JSON (or a
/// future migration) can tell which rules shaped it.
pub const FORMULA_VERSION: u32 = 1;

/// The layout of remembrance.json. Only changes if the file's shape does.
const FILE_VERSION: u32 = 1;

/// How fast "recent" fades: after this long a deed counts half as much.
/// Three days means a good weekend still glows on Monday, a rough week
/// shows by Thursday, and a deed from a week ago counts about a fifth.
/// Shorter (one day) made the blinki moody overnight; longer (two weeks)
/// made it slow to notice you were back to looking after yourself.
pub const HALF_LIFE_SECS: f64 = 3.0 * 24.0 * 3600.0;

/// How much lifetime history it takes for the bond to reach about 63%
/// (1 - 1/e). A "point" is roughly an hour together or a good break; a
/// couple of months of ordinary days gets there, and it keeps creeping up.
const BOND_SCALE: f64 = 250.0;

/// How many lifetime points before the blinki trusts its own memories
/// fully. Until then its moods stay near neutral (0.5), so a brand new
/// blinki isn't judged by an empty ledger.
const FAMILIAR_SCALE: f64 = 15.0;

/// How often to write the file at most, while something changed.
const SAVE_EVERY_SECS: u64 = 120;

/// Seconds since 1970 by the computer's clock. The only place this module
/// reads the real time; everything else is handed `now`.
pub fn unix_now() -> u64 {
    // `duration_since` fails only if the clock is set before 1970;
    // `map(...).unwrap_or(0)` turns that into 0 instead of a panic.
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// How the blinki is, slowly, from what it remembers. All 0..1; 0.5 is
/// neutral for warmth, calm and vigor. `bond` starts at 0 and only grows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Temperament {
    /// Cool and wary (0) to warm and fond (1).
    pub warmth: f32,
    /// Restless and jumpy (0) to settled (1).
    pub calm: f32,
    /// Dim and sleepy (0) to bright and lively (1).
    pub vigor: f32,
    /// How long and how well you have known each other. Never shrinks.
    pub bond: f32,
}

impl Default for Temperament {
    fn default() -> Self {
        Temperament {
            warmth: 0.5,
            calm: 0.5,
            vigor: 0.5,
            bond: 0.0,
        }
    }
}

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
    /// same voice as mind.rs: short, a little eerie, fond underneath.
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

    fn recent(&self, deed: Deed, now: u64) -> f64 {
        self.get(deed).recent_at(now)
    }

    fn count(&self, deed: Deed) -> f64 {
        self.get(deed).count as f64
    }

    /// Lifetime points: every deed ever, weighted like `bond` likes them.
    /// Late nights count too; they are still time spent together.
    fn lifetime_points(&self) -> f64 {
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
    fn recent_care(&self, now: u64) -> f64 {
        let r = |d| self.recent(d, now);
        r(Deed::Break)
            + r(Deed::Rest)
            + 0.5 * r(Deed::Return)
            + 0.5 * r(Deed::Smile)
            + 0.3 * r(Deed::TogetherHour)
            + 0.5 * r(Deed::EarlyMorning)
    }
}

/// A gentle 0..1 curve: 0 at 0, a half at `k`, approaching 1 forever.
/// `x / (x + k)` never overshoots, so no amount of deeds "maxes out".
fn sat(x: f64, k: f64) -> f64 {
    let x = x.max(0.0);
    x / (x + k)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// The temperament formula, version `FORMULA_VERSION`. Pure: the same
/// ledger at the same `now` always gives the same answer.
///
/// - Each mood has a *target* from the recent sums, then is pulled from
///   neutral (0.5) toward it by how *familiar* the blinki is with you
///   (lifetime history). An empty ledger gives exactly the default.
/// - warmth: smiles, returns, time together.
/// - calm: breaks and rest; late nights pull it down a little.
/// - vigor: rest, breaks, returns, smiles, time together. Neglect lets it
///   sink toward sleepy, never to nothing.
/// - bond: lifetime points only, so it can never go down.
pub fn temperament_of(ledger: &Ledger, now: u64) -> Temperament {
    let r = |d| ledger.recent(d, now);
    let familiar = sat(ledger.lifetime_points(), FAMILIAR_SCALE);

    let warmth_target = 0.3
        + 0.7
            * sat(
                r(Deed::Smile)
                    + 0.5 * r(Deed::Return)
                    + 0.25 * r(Deed::TogetherHour)
                    + r(Deed::LongReturn),
                5.0,
            );
    let calm_target = 0.35 + 0.6 * sat(r(Deed::Break) + 2.0 * r(Deed::Rest), 4.0)
        - 0.25 * sat(r(Deed::LateNight), 3.0);
    let vigor_target = 0.25 + 0.75 * sat(ledger.recent_care(now), 6.0);
    let bond = 1.0 - (-ledger.lifetime_points() / BOND_SCALE).exp();

    // `as f32`: the formula works in f64 for precise decay; the struct the
    // rest of the app shares is f32, plenty for colours and speeds.
    let mood = |target: f64| lerp(0.5, target.clamp(0.0, 1.0), familiar) as f32;
    Temperament {
        warmth: mood(warmth_target),
        calm: mood(calm_target),
        vigor: mood(vigor_target),
        bond: bond.clamp(0.0, 1.0) as f32,
    }
}

/// How bright today is, 0..1: 1 when it's been well looked after (or is
/// brand new), sinking toward a floor of 0.45 when it's been left alone.
/// Never dark: a neglected blinki has dim, quiet days, nothing worse.
pub fn day_brightness_of(ledger: &Ledger, now: u64) -> f32 {
    let familiar = sat(ledger.lifetime_points(), FAMILIAR_SCALE);
    let cared = 0.45 + 0.55 * sat(ledger.recent_care(now), 3.0);
    lerp(1.0, cared, familiar) as f32
}

/// Something the blinki has noticed about you over time, from patterns in
/// the ledger. A trait can come and go as habits change, slowly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trait {
    NightOwl,
    EarlyRiser,
    TakesGoodBreaks,
    WellRested,
    Smiley,
    AlwaysComesBack,
}

impl Trait {
    /// Plain words, for a tray tooltip or a whisper.
    pub fn name(self) -> &'static str {
        match self {
            Trait::NightOwl => "a night owl",
            Trait::EarlyRiser => "an early riser",
            Trait::TakesGoodBreaks => "takes good breaks",
            Trait::WellRested => "well rested",
            Trait::Smiley => "smiles a lot",
            Trait::AlwaysComesBack => "always comes back",
        }
    }
}

/// The traits that show in a ledger. Lifetime counts with ratios, so a
/// habit has to be real (several times over) and has to outweigh its
/// opposite: start sleeping early and "night owl" fades as early mornings
/// pile up.
pub fn traits_of(ledger: &Ledger) -> Vec<Trait> {
    let c = |d| ledger.count(d);
    let mut traits = Vec::new();
    if c(Deed::LateNight) >= 5.0 && c(Deed::LateNight) >= 2.0 * c(Deed::EarlyMorning) {
        traits.push(Trait::NightOwl);
    }
    if c(Deed::EarlyMorning) >= 5.0 && c(Deed::EarlyMorning) >= 2.0 * c(Deed::LateNight) {
        traits.push(Trait::EarlyRiser);
    }
    // About one break for every three hours together, or better.
    if c(Deed::Break) >= 10.0 && c(Deed::Break) * 3.0 >= c(Deed::TogetherHour) {
        traits.push(Trait::TakesGoodBreaks);
    }
    if c(Deed::Rest) >= 7.0 && c(Deed::LateNight) * 4.0 <= c(Deed::Rest) {
        traits.push(Trait::WellRested);
    }
    if c(Deed::Smile) >= 20.0 {
        traits.push(Trait::Smiley);
    }
    if c(Deed::Return) >= 30.0 || c(Deed::LongReturn) >= 3.0 {
        traits.push(Trait::AlwaysComesBack);
    }
    traits
}

/// One line in the journal of firsts: what happened, when, and what the
/// blinki said about it. The line is kept as written, so rewording a
/// first line later doesn't rewrite old memories.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub deed: Deed,
    /// Unix seconds.
    pub at: u64,
    pub line: String,
}

/// The whole memory: the ledger, the journal, and when it last saw you.
#[derive(Clone, Debug, PartialEq)]
pub struct Remembrance {
    ledger: Ledger,
    journal: Vec<Entry>,
    /// When it first met you, unix seconds.
    born: u64,
    /// The last moment you were here (wellbeing.rs keeps this fresh). How
    /// long you were gone, even across restarts, is measured from it.
    last_seen: Option<u64>,
    /// Where to save. `None` for a memory that lives only in RAM (tests),
    /// so a test can never scribble over the real file.
    path: Option<PathBuf>,
    /// Something changed since the last save.
    dirty: bool,
    last_save: u64,
    /// The file was written by a newer app. Read what we understand but
    /// never overwrite it, so nothing the newer version kept gets lost.
    read_only: bool,
}

impl Remembrance {
    /// A blank memory that is never saved. For tests, and for when the
    /// user hasn't opted in.
    pub fn fresh(now: u64) -> Remembrance {
        Remembrance {
            ledger: Ledger::default(),
            journal: Vec::new(),
            born: now,
            last_seen: None,
            path: None,
            dirty: false,
            last_save: now,
            read_only: false,
        }
    }

    fn path() -> Option<PathBuf> {
        let base = directories::BaseDirs::new()?;
        Some(
            base.config_dir()
                .join("Unblinking")
                .join("remembrance.json"),
        )
    }

    /// Reads remembrance.json next to prefs.json. Missing means a new
    /// blinki; unreadable means a new blinki too, but the old file is kept
    /// aside as remembrance.json.bad first, in case it can be rescued.
    pub fn load(now: u64) -> Remembrance {
        let Some(path) = Self::path() else {
            return Remembrance::fresh(now);
        };
        let mut memory = match std::fs::read_to_string(&path) {
            Ok(text) => Self::from_json(&text, now).unwrap_or_else(|| {
                let _ = std::fs::copy(&path, path.with_extension("json.bad"));
                Remembrance::fresh(now)
            }),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Remembrance::fresh(now),
            Err(err) => {
                eprintln!("could not read remembrance, starting fresh: {err}");
                Remembrance::fresh(now)
            }
        };
        memory.path = Some(path);
        memory
    }

    /// Parses the file loosely, like prefs.rs: each deed's tally and each
    /// journal entry stands alone, and a bad one is skipped. `None` only if
    /// the text isn't a JSON object at all.
    ///
    /// Note what's *not* read: any saved temperament. Even if a file has
    /// one (an older or hand-edited file might), it's ignored; the
    /// temperament is always the formula's, worked out from the ledger.
    pub fn from_json(text: &str, now: u64) -> Option<Remembrance> {
        let saved: Value = match serde_json::from_str(text) {
            Ok(v @ Value::Object(_)) => v,
            Ok(_) => {
                eprintln!("could not read remembrance: not an object");
                return None;
            }
            Err(err) => {
                eprintln!("could not read remembrance: {err}");
                return None;
            }
        };
        let mut memory = Remembrance::fresh(now);
        let number = |key: &str| saved.get(key).and_then(Value::as_u64);

        if number("version").is_some_and(|v| v > FILE_VERSION as u64) {
            eprintln!("remembrance.json is from a newer Unblinking; reading it, not saving it");
            memory.read_only = true;
        }
        if let Some(born) = number("born") {
            memory.born = born;
        }
        memory.last_seen = number("lastSeen");

        if let Some(ledger) = saved.get("ledger").and_then(Value::as_object) {
            for deed in Deed::ALL {
                let tally = ledger
                    .get(deed.key())
                    .cloned()
                    .and_then(|v| serde_json::from_value::<Tally>(v).ok());
                if let Some(mut tally) = tally {
                    // A hand-edited NaN or negative sum would poison every
                    // formula downstream; a sum above the count is impossible.
                    if !tally.recent.is_finite() || tally.recent < 0.0 {
                        tally.recent = 0.0;
                    }
                    tally.recent = tally.recent.min(tally.count as f64);
                    memory.ledger.tallies[deed as usize] = tally;
                }
            }
        }
        if let Some(entries) = saved.get("journal").and_then(Value::as_array) {
            memory.journal = entries
                .iter()
                .filter_map(|v| serde_json::from_value::<Entry>(v.clone()).ok())
                .collect();
        }
        Some(memory)
    }

    /// The JSON written to disk. The temperament is *not* in it.
    pub fn to_json(&self) -> String {
        // `serde_json::json!` builds a `Value` from JSON-looking Rust.
        let ledger: serde_json::Map<String, Value> = Deed::ALL
            .iter()
            .map(|&d| {
                let tally = serde_json::to_value(self.ledger.get(d)).unwrap_or(Value::Null);
                (d.key().to_string(), tally)
            })
            .collect();
        let value = serde_json::json!({
            "version": FILE_VERSION,
            "formula": FORMULA_VERSION,
            "born": self.born,
            "lastSeen": self.last_seen,
            "ledger": ledger,
            "journal": self.journal,
        });
        serde_json::to_string_pretty(&value).unwrap_or_default()
    }

    /// Writes the file now (if this memory has one and may write it).
    /// Written to a temporary file first and then renamed over the old one,
    /// so a crash mid-write can't leave half a memory behind.
    pub fn save(&mut self, now: u64) {
        self.last_save = now;
        let Some(path) = self.path.clone() else {
            return;
        };
        if self.read_only {
            return;
        }
        let write = || -> std::io::Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let tmp = path.with_extension("json.tmp");
            std::fs::write(&tmp, self.to_json())?;
            std::fs::rename(&tmp, &path)
        };
        match write() {
            Ok(()) => self.dirty = false,
            Err(err) => eprintln!("could not save remembrance: {err}"),
        }
    }

    /// Saves if something changed and it's been a couple of minutes. Cheap
    /// to call every frame. Call `save` once more when the app quits.
    pub fn save_if_due(&mut self, now: u64) {
        if self.dirty && now.saturating_sub(self.last_save) >= SAVE_EVERY_SECS {
            self.save(now);
        }
    }

    /// Notes that something happened. Returns the journal entry if this is
    /// the first time ever, so the app can whisper `entry.line`.
    pub fn record(&mut self, deed: Deed, now: u64) -> Option<Entry> {
        let first = self.ledger.get(deed).count == 0;
        self.ledger.add(deed, now);
        self.dirty = true;
        eprintln!("remembered: {}", deed.key());
        if !first {
            return None;
        }
        let entry = Entry {
            deed,
            at: now,
            line: deed.first_line().to_string(),
        };
        self.journal.push(entry.clone());
        Some(entry)
    }

    /// You're here right now.
    pub fn mark_seen(&mut self, now: u64) {
        if self.last_seen != Some(now) {
            self.last_seen = Some(now);
            self.dirty = true;
        }
    }

    pub fn last_seen(&self) -> Option<u64> {
        self.last_seen
    }

    /// When the blinki first met you, unix seconds.
    pub fn born(&self) -> u64 {
        self.born
    }

    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    /// The firsts, oldest first.
    pub fn journal(&self) -> &[Entry] {
        &self.journal
    }

    /// How it is, right now, from what it remembers.
    pub fn temperament(&self, now: u64) -> Temperament {
        temperament_of(&self.ledger, now)
    }

    /// How bright its days are, 0.45..1. See `day_brightness_of`.
    pub fn day_brightness(&self, now: u64) -> f32 {
        day_brightness_of(&self.ledger, now)
    }

    pub fn traits(&self) -> Vec<Trait> {
        traits_of(&self.ledger)
    }

    /// Forgets everything and deletes the file: for "forget me" in the
    /// menu, or turning Remembrance off. Fully, not "hidden".
    pub fn forget(&mut self, now: u64) {
        // Even a newer app's file goes: "forget" means forget.
        let path = self.path.take();
        if let Some(p) = &path {
            let _ = std::fs::remove_file(p);
        }
        *self = Remembrance::fresh(now);
        self.path = path;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn an_empty_ledger_is_the_default_temperament() {
        assert_eq!(
            temperament_of(&Ledger::default(), T0),
            Temperament::default()
        );
        assert_eq!(day_brightness_of(&Ledger::default(), T0), 1.0);
    }

    #[test]
    fn only_the_first_of_each_deed_is_journaled() {
        let mut m = Remembrance::fresh(T0);
        let first = m.record(Deed::Smile, T0).expect("first smile is a first");
        assert_eq!(first.deed, Deed::Smile);
        assert_eq!(first.at, T0);
        assert_eq!(first.line, Deed::Smile.first_line());
        assert_eq!(m.record(Deed::Smile, T0 + 60), None);
        assert!(m.record(Deed::Break, T0 + 120).is_some());
        assert_eq!(m.journal().len(), 2);
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

    #[test]
    fn bond_never_shrinks() {
        let mut m = Remembrance::fresh(T0);
        let mut now = T0;
        let mut last = 0.0;
        fastrand::seed(7);
        for _ in 0..500 {
            // Random deeds at random gaps, some of them weeks long.
            now += fastrand::u64(0..(10 * DAY));
            m.record(Deed::ALL[fastrand::usize(0..Deed::COUNT)], now);
            let bond = m.temperament(now).bond;
            assert!(bond >= last, "bond fell from {last} to {bond}");
            last = bond;
        }
        assert!(last > 0.5 && last < 1.0);
    }

    #[test]
    fn care_warms_and_neglect_dims_but_never_darkens() {
        let mut m = Remembrance::fresh(T0);
        // Two good weeks: rest each night, breaks, smiles, hours together.
        for day in 0..14 {
            let t = T0 + day * DAY;
            m.record(Deed::Rest, t);
            for h in 0..6 {
                m.record(Deed::TogetherHour, t + h * 3600);
            }
            m.record(Deed::Break, t + 3 * 3600);
            m.record(Deed::Smile, t + 4 * 3600);
        }
        let good = T0 + 14 * DAY;
        let cared = m.temperament(good);
        assert!(
            cared.warmth > 0.6 && cared.calm > 0.6 && cared.vigor > 0.6,
            "{cared:?}"
        );
        assert!(m.day_brightness(good) > 0.85);

        // Then a month alone.
        let later = good + 30 * DAY;
        let alone = m.temperament(later);
        assert!(alone.vigor < cared.vigor && alone.warmth < cared.warmth);
        assert_eq!(alone.bond, cared.bond, "time apart doesn't cost the bond");
        let dim = m.day_brightness(later);
        assert!((0.45..0.6).contains(&dim), "dim, not dark: {dim}");
    }

    #[test]
    fn late_nights_cost_a_little_calm() {
        let mut a = Remembrance::fresh(T0);
        let mut b = Remembrance::fresh(T0);
        for day in 0..5 {
            let t = T0 + day * DAY;
            a.record(Deed::Break, t);
            b.record(Deed::Break, t);
            b.record(Deed::LateNight, t + 60);
        }
        let now = T0 + 5 * DAY;
        let (ca, cb) = (a.temperament(now).calm, b.temperament(now).calm);
        assert!(cb < ca, "late nights lower calm: {cb} vs {ca}");
        assert!(cb > 0.4, "gently: {cb}");
    }

    #[test]
    fn traits_follow_patterns() {
        let mut m = Remembrance::fresh(T0);
        assert!(m.traits().is_empty());
        for day in 0..6 {
            m.record(Deed::LateNight, T0 + day * DAY);
        }
        assert_eq!(m.traits(), vec![Trait::NightOwl]);
        // Habits change: enough early mornings and the owl is gone.
        for day in 6..20 {
            m.record(Deed::EarlyMorning, T0 + day * DAY);
        }
        assert_eq!(m.traits(), vec![Trait::EarlyRiser]);
    }

    #[test]
    fn round_trips_through_json() {
        let mut m = Remembrance::fresh(T0);
        m.record(Deed::Break, T0 + 10);
        m.record(Deed::Break, T0 + 20);
        m.record(Deed::Smile, T0 + 30);
        m.mark_seen(T0 + 40);
        let back = Remembrance::from_json(&m.to_json(), T0 + 50).unwrap();
        assert_eq!(back.ledger, m.ledger);
        assert_eq!(back.journal, m.journal);
        assert_eq!(back.born, T0);
        assert_eq!(back.last_seen, Some(T0 + 40));
    }

    #[test]
    fn the_temperament_is_recomputed_not_read() {
        let mut m = Remembrance::fresh(T0);
        for i in 0..10 {
            m.record(Deed::Smile, T0 + i * 600);
        }
        // An old file: formula 0, and a stored temperament that disagrees
        // with its own ledger. The stored one must be ignored.
        let mut file: Value = serde_json::from_str(&m.to_json()).unwrap();
        assert_eq!(file["formula"], FORMULA_VERSION);
        assert!(file.get("temperament").is_none(), "never saved");
        file["formula"] = Value::from(0);
        file["temperament"] = serde_json::json!({ "warmth": 0.0, "calm": 0.0 });
        let old = Remembrance::from_json(&file.to_string(), T0).unwrap();
        let now = T0 + DAY;
        assert_eq!(old.temperament(now), m.temperament(now));
        assert_eq!(old.temperament(now), temperament_of(&m.ledger, now));
    }

    #[test]
    fn garbled_files_fall_back_without_crashing() {
        assert!(Remembrance::from_json("not json", T0).is_none());
        assert!(Remembrance::from_json("[1, 2]", T0).is_none());
        let empty = Remembrance::from_json("{}", T0).unwrap();
        assert_eq!(empty.ledger, Ledger::default());
        assert!(empty.journal.is_empty());

        // One bad tally, one bad journal entry, a poisoned sum: each spoils
        // only itself.
        let m = Remembrance::from_json(
            r#"{ "version": 1, "born": 5,
                 "ledger": { "break": { "count": "lots" },
                             "smile": { "count": 3, "recent": 2.5, "at": 100 },
                             "rest":  { "count": 2, "recent": -4 },
                             "return": { "count": 1, "recent": 99 } },
                 "journal": [ { "deed": "smile", "at": 7, "line": "hi" },
                              { "deed": "dancing", "at": 8, "line": "?" } ] }"#,
            T0,
        )
        .unwrap();
        assert_eq!(m.born, 5);
        assert_eq!(m.ledger.get(Deed::Break).count, 0);
        assert_eq!(m.ledger.get(Deed::Smile).count, 3);
        assert_eq!(m.ledger.get(Deed::Rest).recent, 0.0);
        assert_eq!(
            m.ledger.get(Deed::Return).recent,
            1.0,
            "never above the count"
        );
        assert_eq!(m.journal.len(), 1);
        assert!(!m.read_only);
    }

    #[test]
    fn a_newer_file_is_read_but_not_overwritten() {
        let m = Remembrance::from_json(r#"{ "version": 99, "born": 5 }"#, T0).unwrap();
        assert!(m.read_only);
        assert_eq!(m.born, 5);
    }

    #[test]
    fn save_and_load_through_a_real_file() {
        let dir =
            std::env::temp_dir().join(format!("unblinking-remembrance-{}", std::process::id()));
        let path = dir.join("remembrance.json");
        let mut m = Remembrance::fresh(T0);
        m.path = Some(path.clone());
        m.record(Deed::Rest, T0);
        m.save_if_due(T0 + 1);
        assert!(!path.exists(), "not due yet");
        m.save_if_due(T0 + SAVE_EVERY_SECS);
        assert!(path.exists() && !m.dirty);
        let text = std::fs::read_to_string(&path).unwrap();
        let back = Remembrance::from_json(&text, T0).unwrap();
        assert_eq!(back.ledger, m.ledger);

        m.forget(T0 + DAY);
        assert!(!path.exists(), "forget deletes the file");
        assert_eq!(m.ledger, Ledger::default());
        assert_eq!(m.path, Some(path));
        let _ = std::fs::remove_dir_all(dir);
    }
}
