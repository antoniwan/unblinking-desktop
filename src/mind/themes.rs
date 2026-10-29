//! Themes: who the blinki *is*. A theme is the whole character: its
//! material, its palette, its whisper font, its kind of fire, and its own
//! take on each of the eight moods. The layers stack like this:
//!
//!   Theme  who it is        (The Burning Eye; more to come)
//!   Mood   how it feels now (Watchful, Curious, ... one of the studio's 8)
//!   Mode   what it's doing  (locked on, hunting, blind: mind.rs, internal)
//!
//! So "Wrathful" in The Burning Eye is a white-hot roar, and in some later
//! theme it might be a cold glare: same feeling, different creature.
//!
//! The themes come from assets/blinki.json, exported by the Blinki Studio.

use std::sync::OnceLock;

use serde::Deserialize;

use crate::mind::moods::Mood;

/// The studio's export, built into the exe.
static BLINKI_JSON: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/blinki.json"));

/// The theme a new blinki starts as.
pub const DEFAULT: &str = "burning-eye";

/// The eye shaders this build can draw. A theme naming any other shader
/// (say, one being designed in the studio) is kept but not offered yet.
const SHADERS: &[&str] = &["fire"];

/// The nine moods every theme should have, in the studio's order.
#[allow(dead_code)]
pub const MOOD_NAMES: [&str; 9] = [
    "Watchful",
    "Curious",
    "Lonely",
    "Wrathful",
    "Delighted",
    "Suspicious",
    "Drowsy",
    "Blinded",
    "Worried",
];

/// One theme, as the studio exports it.
///
/// `#[serde(default)]` on a field means "if the JSON leaves it out, use the
/// type's default" (an empty String, None, ...), so older exports that lack
/// a field still load. `rename_all = "camelCase"` matches the studio's
/// `fireStyle` to our `fire_style`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Theme {
    /// A short, stable name for saving in prefs, like "burning-eye".
    #[serde(default)]
    pub id: String,
    /// What the menu shows, like "The Burning Eye".
    pub name: String,
    /// A line about it, for a theme picker later.
    #[serde(default)]
    #[allow(dead_code)]
    pub about: String,
    /// Which eye shader draws it. None (or one this build lacks) means the
    /// theme can't be shown yet.
    #[serde(default)]
    pub shader: Option<String>,
    /// The studio's flame recipe, passed on to the shader.
    #[serde(default)]
    #[allow(dead_code)]
    pub fire_style: String,
    /// The whisper font's family name.
    #[serde(default)]
    #[allow(dead_code)]
    pub font: String,
    pub moods: Vec<Mood>,
}

impl Theme {
    /// Can this build draw it?
    pub fn drawable(&self) -> bool {
        self.shader.as_deref().is_some_and(|s| SHADERS.contains(&s))
    }

    /// The theme's own version of the mood called `name`.
    #[allow(dead_code)]
    pub fn mood(&self, name: &str) -> Option<&Mood> {
        self.moods.iter().find(|m| m.name == name)
    }
}

/// The two shapes blinki.json has had. `#[serde(untagged)]` tells serde to
/// try each variant in turn and keep the first that fits, so files from
/// before themes (one flat theme) still load.
#[derive(Deserialize)]
#[serde(untagged)]
enum Export {
    /// Today's shape: `{ "themes": [ {...}, ... ] }`.
    Themes { themes: Vec<Theme> },
    /// The older shape: one theme's fields at the top level, no id.
    Flat(Theme),
}

/// Every theme the app knows.
#[derive(Debug)]
pub struct Themes {
    list: Vec<Theme>,
}

impl Themes {
    /// The themes built into the exe, parsed once on first use.
    ///
    /// `OnceLock` is a box that's filled at most once, safely, even with
    /// several threads asking. Because it's a `static`, what it holds lives
    /// for the whole run, so we can hand out `&'static` references to it.
    pub fn builtin() -> &'static Themes {
        static THEMES: OnceLock<Themes> = OnceLock::new();
        THEMES.get_or_init(|| {
            // The file is ours and checked by a test, so a failure is a bug.
            Themes::parse(BLINKI_JSON).expect("assets/blinki.json is valid")
        })
    }

    /// Reads a studio export, in either shape. Themes without moods are
    /// dropped, and a theme without an id gets one made from its name.
    pub fn parse(text: &str) -> Result<Themes, String> {
        let export: Export = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let list = match export {
            Export::Themes { themes } => themes,
            // The pre-theme export was always the fire eye.
            Export::Flat(mut theme) => {
                if theme.shader.is_none() {
                    theme.shader = Some("fire".into());
                }
                vec![theme]
            }
        };
        let list: Vec<Theme> = list
            .into_iter()
            .filter(|t| !t.moods.is_empty())
            .map(|mut t| {
                if t.id.is_empty() {
                    t.id = slug(&t.name);
                }
                t
            })
            .collect();
        if list.is_empty() {
            return Err("no themes with moods".into());
        }
        Ok(Themes { list })
    }

    /// All of them, drawable or not.
    #[allow(dead_code)]
    pub fn all(&self) -> &[Theme] {
        &self.list
    }

    /// The ones this build can draw, for the menu.
    pub fn drawable(&self) -> impl Iterator<Item = &Theme> {
        self.list.iter().filter(|t| t.drawable())
    }

    /// The theme with this id, if there is one.
    pub fn get(&self, id: &str) -> Option<&Theme> {
        self.list.iter().find(|t| t.id == id)
    }

    /// The user's pick if it exists and can be drawn, otherwise the
    /// default, otherwise whatever comes first. Never fails, so a stale or
    /// hand-edited pref can't stop the eye from opening.
    pub fn pick(&self, id: Option<&str>) -> &Theme {
        id.and_then(|id| self.get(id))
            .filter(|t| t.drawable())
            .or_else(|| self.get(DEFAULT))
            .or_else(|| self.drawable().next())
            .unwrap_or(&self.list[0])
    }
}

/// "The Burning Eye" to "the-burning-eye".
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_burning_eye_ships_with_all_eight_moods() {
        let themes = Themes::builtin();
        let eye = themes.get(DEFAULT).expect("the default theme exists");
        assert_eq!(eye.name, "The Burning Eye");
        assert!(eye.drawable());
        assert_eq!(eye.font, "Tiny5");
        assert_eq!(eye.fire_style, "v2");
        let names: Vec<_> = eye.moods.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, MOOD_NAMES, "every mood, in the studio's order");
    }

    #[test]
    fn every_theme_has_every_mood() {
        for theme in Themes::builtin().all() {
            for name in MOOD_NAMES {
                assert!(theme.mood(name).is_some(), "{} lacks {name}", theme.name);
            }
        }
    }

    #[test]
    fn public_names_stay_our_own() {
        for theme in Themes::builtin().all() {
            assert!(!theme.name.to_lowercase().contains("sauron"));
        }
    }

    #[test]
    fn the_moods_numbers_survived_the_move() {
        // Spot checks against the flat export this file used to be.
        let eye = Themes::builtin().get(DEFAULT).unwrap();
        let wrath = eye.mood("Wrathful").unwrap();
        assert_eq!(wrath.heat, 1.2);
        assert_eq!(wrath.shake, 0.6);
        assert_eq!(wrath.colors[0], "#a00000");
        let watch = eye.mood("Watchful").unwrap();
        assert_eq!((watch.heat, watch.flow, watch.swirl), (0.55, 0.08, 3.2));
        assert_eq!(eye.mood("Blinded").unwrap().dim, 0.5);
    }

    #[test]
    fn the_old_flat_export_still_loads() {
        let flat = r##"{ "name": "OG Blinki", "fireStyle": "v2", "font": "Tiny5",
            "moods": [ { "name": "Watchful", "line": "I see you.",
              "colors": ["#8a1400", "#ff6a00", "#ffd27a", "#ff5a14"],
              "dim": 0, "embers": 0.5, "flow": 0.08, "glow": 1, "heat": 0.55,
              "pupil": 0.15, "shake": 0, "swirl": 3.2, "wake": 1 } ] }"##;
        let themes = Themes::parse(flat).unwrap();
        let t = &themes.all()[0];
        assert_eq!(t.id, "og-blinki");
        assert!(t.drawable(), "the old export was always the fire eye");
        assert_eq!(t.mood("Watchful").unwrap().heat, 0.55);
    }

    #[test]
    fn a_theme_this_build_cannot_draw_is_kept_but_not_offered() {
        let json = r##"{ "themes": [
            { "id": "glass", "name": "Glass", "shader": "glass", "moods": [
              { "name": "Watchful", "line": "", "colors": ["#000","#000","#000","#000"],
                "dim": 0, "embers": 0, "flow": 0, "glow": 0, "heat": 0, "shake": 0,
                "swirl": 0, "wake": 0 } ] },
            { "id": "fire", "name": "Fire", "shader": "fire", "moods": [
              { "name": "Watchful", "line": "", "colors": ["#000","#000","#000","#000"],
                "dim": 0, "embers": 0, "flow": 0, "glow": 0, "heat": 0, "shake": 0,
                "swirl": 0, "wake": 0 } ] } ] }"##;
        let themes = Themes::parse(json).unwrap();
        assert_eq!(themes.all().len(), 2);
        let offered: Vec<_> = themes.drawable().map(|t| t.id.as_str()).collect();
        assert_eq!(offered, ["fire"]);
        assert_eq!(
            themes.pick(Some("glass")).id,
            "fire",
            "can't draw it, so no"
        );
    }

    #[test]
    fn picking_falls_back_to_the_burning_eye() {
        let themes = Themes::builtin();
        assert_eq!(themes.pick(None).id, DEFAULT);
        assert_eq!(themes.pick(Some("no-such-theme")).id, DEFAULT);
        assert_eq!(themes.pick(Some(DEFAULT)).id, DEFAULT);
    }

    #[test]
    fn junk_is_an_error_not_a_panic() {
        assert!(Themes::parse("not json").is_err());
        assert!(Themes::parse(r#"{ "themes": [] }"#).is_err());
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("The Burning Eye"), "the-burning-eye");
        assert_eq!(slug("  OG  Blinki! "), "og-blinki");
    }
}
