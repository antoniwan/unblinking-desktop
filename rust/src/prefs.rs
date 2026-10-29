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
    /// The "Look at me" calibration, in metres (Rust app only).
    pub look_offset: Option<Position>,
    /// Music, Voice and FX, each with a mute and a volume (Rust app only).
    /// Electron's "voice" key is kept in step with it.
    pub sound: crate::sound::Sound,
    /// Where the webcam sits, from "Where's my camera?" (Rust app only).
    pub camera: Option<crate::placement::CameraSpot>,
    /// Get out of the way of full-screen games, videos and presentations
    /// (Rust app only). On by default.
    pub hide_fullscreen: bool,
    /// Listen for "Hey, OG Blinki" (Rust app only). Off by default.
    pub wake_word: bool,
    /// What you call your blinki, if you've named it (Rust app only).
    pub name: Option<String>,
    /// Which theme it is, by id (see themes.rs). None is The Burning Eye.
    pub theme: Option<String>,
    /// "Read my face": read smiles and where you look, on this computer
    /// only. Off unless you turn it on.
    pub feel: bool,
    /// "Remember our days": keep the Remembrance ledger on this computer.
    /// Off unless you turn it on.
    pub remember: bool,
}

/// A name longer than this is cut, so the menu header stays a header.
const NAME_MAX_CHARS: usize = 32;

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            whispers: true,
            blind: false,
            click_through: false,
            voice: false,
            position: None,
            look_offset: None,
            camera: None,
            hide_fullscreen: true,
            wake_word: false,
            name: None,
            theme: None,
            feel: false,
            remember: false,
            sound: crate::sound::Sound::default(),
        }
    }
}

/// Tidies a name: trimmed, at most NAME_MAX_CHARS characters, and None if
/// nothing is left. Counting `chars()` (not bytes) keeps a name like
/// "Ëmber" or "炎" from being cut in the middle of a letter.
fn tidy_name(raw: &str) -> Option<String> {
    let name: String = raw.trim().chars().take(NAME_MAX_CHARS).collect();
    let name = name.trim_end().to_string();
    (!name.is_empty()).then_some(name)
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
        if let Some(v) = flag("hideFullscreen") {
            prefs.hide_fullscreen = v;
        }
        if let Some(v) = flag("wakeWord") {
            prefs.wake_word = v;
        }
        if let Some(v) = flag("feel") {
            prefs.feel = v;
        }
        if let Some(v) = flag("remember") {
            prefs.remember = v;
        }
        // Text keys: kept only if they're strings with something in them.
        let text = |key: &str| saved.get(key).and_then(Value::as_str);
        prefs.name = text("name").and_then(tidy_name);
        prefs.theme = text("theme")
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(String::from);
        // Deserialize just these fields; a malformed one is dropped.
        let point = |key: &str| {
            saved
                .get(key)
                .cloned()
                .and_then(|v| serde_json::from_value::<Position>(v).ok())
                .filter(|p| p.x.is_finite() && p.y.is_finite())
        };
        prefs.position = point("position");
        prefs.look_offset = point("lookOffset");
        // No mixer saved yet: carry the old single Voice toggle over.
        prefs.sound = saved
            .get("sound")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_else(|| crate::sound::Sound::from_old_voice(prefs.voice));
        prefs.camera = saved
            .get("camera")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok());
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
    fn the_new_keys_default_to_off_and_unnamed() {
        let p = Prefs::default();
        assert!(
            !p.feel && !p.remember,
            "nothing is read or kept unless asked"
        );
        assert_eq!((p.name, p.theme), (None, None));
    }

    #[test]
    fn reads_name_theme_and_the_opt_ins() {
        let p = Prefs::from_json(
            r#"{ "name": "  Ember ", "theme": "burning-eye", "feel": true, "remember": true }"#,
        );
        assert_eq!(p.name.as_deref(), Some("Ember"));
        assert_eq!(p.theme.as_deref(), Some("burning-eye"));
        assert!(p.feel && p.remember);
    }

    #[test]
    fn bad_names_and_themes_are_dropped() {
        let p = Prefs::from_json(r#"{ "name": "   ", "theme": 7, "feel": "yes", "remember": 1 }"#);
        assert_eq!(p, Prefs::default());
        let long = "x".repeat(100);
        let p = Prefs::from_json(&format!(r#"{{ "name": "{long}" }}"#));
        assert_eq!(p.name.map(|n| n.chars().count()), Some(NAME_MAX_CHARS));
        assert_eq!(tidy_name("炎の目").as_deref(), Some("炎の目"));
    }

    #[test]
    fn round_trips_with_js_names() {
        let p = Prefs {
            click_through: true,
            position: Some(Position { x: 1.0, y: 2.0 }),
            name: Some("Ember".into()),
            theme: Some("burning-eye".into()),
            feel: true,
            remember: true,
            ..Prefs::default()
        };
        let text = serde_json::to_string(&p).unwrap();
        assert!(text.contains("\"clickThrough\":true"), "{text}");
        assert!(text.contains("\"wakeWord\":false"), "{text}");
        assert_eq!(Prefs::from_json(&text), p);
    }

    #[test]
    fn hide_fullscreen_defaults_on() {
        assert!(
            Prefs::from_json("{}").hide_fullscreen,
            "missing key means on"
        );
        let off = Prefs {
            hide_fullscreen: false,
            ..Prefs::default()
        };
        let text = serde_json::to_string(&off).unwrap();
        assert!(text.contains("\"hideFullscreen\":false"), "{text}");
        assert!(!Prefs::from_json(&text).hide_fullscreen);
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let p = Prefs::from_json(r#"{ "whispers": false, "colour": "dark", "future": [1, 2] }"#);
        let want = Prefs {
            whispers: false,
            ..Prefs::default()
        };
        assert_eq!(p, want);
    }

    #[test]
    fn rust_only_keys_load_and_bad_ones_drop() {
        use crate::placement::Edge;
        let p = Prefs::from_json(
            r#"{ "lookOffset": { "x": 0.02, "y": -0.01 }, "position": null,
                 "camera": { "monitor": "DISPLAY2", "edge": "left", "offset": 0.25 } }"#,
        );
        assert_eq!(p.look_offset, Some(Position { x: 0.02, y: -0.01 }));
        assert_eq!(p.position, None);
        let cam = p.camera.expect("camera spot");
        assert_eq!((cam.monitor.as_str(), cam.edge), ("DISPLAY2", Edge::Left));

        let p = Prefs::from_json(
            r#"{ "voice": true, "camera": { "monitor": "X", "edge": "diagonal", "offset": 0.5 } }"#,
        );
        assert_eq!(p.camera, None, "an unknown edge drops just the camera");
        assert!(p.voice);
    }

    #[test]
    fn everything_round_trips() {
        use crate::placement::{CameraSpot, Edge};
        let p = Prefs {
            whispers: false,
            blind: true,
            click_through: true,
            voice: true,
            position: Some(Position { x: -300.5, y: 12.0 }),
            look_offset: Some(Position { x: 0.01, y: 0.03 }),
            camera: Some(CameraSpot {
                monitor: "DISPLAY1".into(),
                edge: Edge::Bottom,
                offset: 0.7,
            }),
            hide_fullscreen: false,
            wake_word: true,
            sound: crate::sound::Sound::default(),
            name: Some("Lucero".into()),
            theme: Some("burning-eye".into()),
            feel: true,
            remember: true,
        };
        let text = serde_json::to_string_pretty(&p).unwrap();
        assert_eq!(Prefs::from_json(&text), p);
        let d = Prefs::default();
        assert_eq!(Prefs::from_json(&serde_json::to_string(&d).unwrap()), d);
    }
}
