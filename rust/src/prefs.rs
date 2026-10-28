//! Everything the user can toggle, saved as JSON. Same file, keys and
//! defaults as the Electron app's prefs.json, so both versions share them:
//! %APPDATA%\Unblinking\prefs.json on Windows,
//! ~/Library/Application Support/Unblinking/prefs.json on a Mac.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Where the window's top left corner was last dragged, in logical pixels
/// (what Electron saved), so the spot is the same at any display scale.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

/// `#[derive(Serialize)]` writes the code that turns this struct into JSON;
/// `rename_all` keeps the JS names (`clickThrough`, not `click_through`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prefs {
    pub whispers: bool,
    /// Camera and mic off.
    pub blind: bool,
    pub click_through: bool,
    /// Speak whispers and let the eye hum. Off by default.
    pub voice: bool,
    pub position: Option<Position>,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            whispers: true,
            blind: false,
            click_through: false,
            voice: false,
            position: None,
        }
    }
}

impl Prefs {
    fn path() -> Option<PathBuf> {
        // `?` works on Option too: no config folder means no path.
        let base = directories::BaseDirs::new()?;
        Some(base.config_dir().join("Unblinking").join("prefs.json"))
    }

    /// Reads the saved prefs. A missing or hand-mangled file falls back to
    /// the defaults, key by key, as main.js did.
    pub fn load() -> Prefs {
        let Some(path) = Self::path() else {
            return Prefs::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::from_json(&text),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Prefs::default(),
            Err(err) => {
                eprintln!("could not read prefs, using defaults: {err}");
                Prefs::default()
            }
        }
    }

    /// Parses JSON loosely: every key that is there and the right type wins,
    /// everything else keeps its default. That's why this reads a generic
    /// `serde_json::Value` instead of deriving `Deserialize` for the struct,
    /// which would reject the whole file over one bad key.
    pub fn from_json(text: &str) -> Prefs {
        let mut prefs = Prefs::default();
        let saved: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(err) => {
                eprintln!("could not read prefs, using defaults: {err}");
                return prefs;
            }
        };
        let flag = |key: &str| saved.get(key).and_then(Value::as_bool);
        // `if let Some(v) = ...` sets the pref only when the key was usable.
        if let Some(v) = flag("whispers") {
            prefs.whispers = v;
        }
        if let Some(v) = flag("blind") {
            prefs.blind = v;
        }
        if let Some(v) = flag("clickThrough") {
            prefs.click_through = v;
        }
        if let Some(v) = flag("voice") {
            prefs.voice = v;
        }
        // Deserialize just this one field; a malformed position is dropped.
        if let Some(pos) = saved.get("position").cloned() {
            prefs.position = serde_json::from_value::<Position>(pos)
                .ok()
                .filter(|p| p.x.is_finite() && p.y.is_finite());
        }
        prefs
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        // A closure that returns Result lets us use `?` for each step and
        // report any failure in one place.
        let write = || -> std::io::Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
            std::fs::write(&path, text)
        };
        if let Err(err) = write() {
            eprintln!("could not save prefs: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_keys_keep_their_defaults() {
        assert_eq!(Prefs::from_json("{}"), Prefs::default());
        assert_eq!(Prefs::from_json("not json"), Prefs::default());
    }

    #[test]
    fn reads_the_electron_file() {
        let p = Prefs::from_json(
            r#"{ "whispers": false, "blind": true, "clickThrough": true, "voice": true,
                 "position": { "x": 1500, "y": 24 } }"#,
        );
        assert!(!p.whispers && p.blind && p.click_through && p.voice);
        assert_eq!(p.position, Some(Position { x: 1500.0, y: 24.0 }));
    }

    #[test]
    fn a_bad_key_spoils_only_itself() {
        let p = Prefs::from_json(
            r#"{ "whispers": "yes", "blind": true, "position": { "x": "left" } }"#,
        );
        assert!(p.whispers, "wrong type keeps the default");
        assert!(p.blind);
        assert_eq!(p.position, None);
    }

    #[test]
    fn round_trips_with_js_names() {
        let p = Prefs {
            click_through: true,
            position: Some(Position { x: 1.0, y: 2.0 }),
            ..Prefs::default()
        };
        let text = serde_json::to_string(&p).unwrap();
        assert!(text.contains("\"clickThrough\":true"), "{text}");
        assert_eq!(Prefs::from_json(&text), p);
    }
}
