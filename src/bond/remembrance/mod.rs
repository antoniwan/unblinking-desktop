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
//! A few rules this module keeps (the ledger in ledger.rs, the formulas
//! in temperament.rs, the memory and its file here):
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

mod ledger;
mod temperament;

// Re-exported so callers keep writing `remembrance::Deed`,
// `remembrance::Temperament` and so on, whichever file they live in.
pub use ledger::{Deed, Ledger, Tally};
pub use temperament::{
    FORMULA_VERSION, Temperament, Trait, day_brightness_of, temperament_of, traits_of,
};

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The layout of remembrance.json. Only changes if the file's shape does.
const FILE_VERSION: u32 = 1;

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
                    memory.ledger.set(deed, tally);
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
    #[allow(dead_code)] // for a "together since" line later
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
