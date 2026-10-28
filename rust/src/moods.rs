//! Moods: the colours and feel of each of the blinki's states, as tuned in
//! the Blinki Studio and exported to assets/blinki.json. The mind decides
//! *what* the eye does; the mood decides how the fire looks while it does it.

use std::time::Instant;

use serde::Deserialize;

/// The studio's export, built into the exe. `Deserialize` is derived: serde
/// writes the code that fills these structs from JSON, matching field names.
static BLINKI_JSON: &str = include_str!("../assets/blinki.json");

/// How long one mood takes to melt into the next (the studio's BLEND).
const BLEND_SECS: f32 = 1.4;

#[derive(Clone, Debug, Deserialize)]
pub struct Mood {
    pub name: String,
    /// What it whispers in this mood. Used once whispers are on screen.
    #[allow(dead_code)]
    pub line: String,
    /// Deep fire, flame, hot core, glow, as "#rrggbb".
    pub colors: [String; 4],
    pub dim: f32,
    pub embers: f32,
    /// Flame drift speed, in loops per second.
    pub flow: f32,
    pub glow: f32,
    pub heat: f32,
    pub shake: f32,
    pub swirl: f32,
    pub wake: f32,
}

#[derive(Deserialize)]
struct Blinki {
    moods: Vec<Mood>,
}

/// One mood as numbers the shader can use, with colours in linear light so
/// that blending two of them doesn't go muddy in the middle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub colors: [[f32; 3]; 4],
    pub dim: f32,
    pub embers: f32,
    pub flow: f32,
    pub glow: f32,
    pub heat: f32,
    pub shake: f32,
    pub swirl: f32,
    pub wake: f32,
}

impl Look {
    fn of(mood: &Mood) -> Look {
        Look {
            colors: mood.colors.each_ref().map(|c| hex(c).map(to_linear)),
            dim: mood.dim,
            embers: mood.embers,
            flow: mood.flow,
            glow: mood.glow,
            heat: mood.heat,
            shake: mood.shake,
            swirl: mood.swirl,
            wake: mood.wake,
        }
    }

    fn mix(a: &Look, b: &Look, t: f32) -> Look {
        let m = |x: f32, y: f32| x + (y - x) * t;
        let mut colors = a.colors;
        for (c, (ca, cb)) in colors.iter_mut().zip(a.colors.iter().zip(&b.colors)) {
            for i in 0..3 {
                c[i] = m(ca[i], cb[i]);
            }
        }
        Look {
            colors,
            dim: m(a.dim, b.dim),
            embers: m(a.embers, b.embers),
            flow: m(a.flow, b.flow),
            glow: m(a.glow, b.glow),
            heat: m(a.heat, b.heat),
            shake: m(a.shake, b.shake),
            swirl: m(a.swirl, b.swirl),
            wake: m(a.wake, b.wake),
        }
    }

    /// The colours as the shader wants them: back from linear to display values.
    pub fn display_colors(&self) -> [[f32; 3]; 4] {
        self.colors.map(|c| c.map(from_linear))
    }
}

/// "#ff6a00" to [1.0, 0.42, 0.0]. A malformed colour comes out black.
fn hex(s: &str) -> [f32; 3] {
    let n = u32::from_str_radix(s.trim_start_matches('#'), 16).unwrap_or(0);
    [(n >> 16) & 255, (n >> 8) & 255, n & 255].map(|v| v as f32 / 255.0)
}

fn to_linear(v: f32) -> f32 {
    v.powf(2.2)
}

fn from_linear(v: f32) -> f32 {
    v.max(0.0).powf(1.0 / 2.2)
}

/// All the moods, and a smooth blend toward whichever one is current.
pub struct Moods {
    moods: Vec<Mood>,
    current: usize,
    from: Look,
    to: Look,
    since: Instant,
}

impl Moods {
    pub fn load() -> Moods {
        // The file is ours and checked by a test, so a parse failure is a bug.
        let blinki: Blinki =
            serde_json::from_str(BLINKI_JSON).expect("assets/blinki.json is valid");
        assert!(!blinki.moods.is_empty(), "blinki.json has no moods");
        let first = Look::of(&blinki.moods[0]);
        Moods {
            moods: blinki.moods,
            current: 0,
            from: first,
            to: first,
            since: Instant::now(),
        }
    }

    /// Starts blending toward the mood called `name` (if it exists and isn't current).
    pub fn set(&mut self, name: &str) {
        let Some(i) = self.moods.iter().position(|m| m.name == name) else {
            return;
        };
        if i == self.current {
            return;
        }
        self.from = self.now();
        self.to = Look::of(&self.moods[i]);
        self.current = i;
        self.since = Instant::now();
        eprintln!("mood: {name}");
    }

    /// The look right now, partway between the last mood and the current one.
    pub fn now(&self) -> Look {
        let t = (self.since.elapsed().as_secs_f32() / BLEND_SECS).min(1.0);
        let eased = t * t * (3.0 - 2.0 * t); // smoothstep, as the studio does
        Look::mix(&self.from, &self.to, eased)
    }

    /// How far the blend into the current mood has got, 0..1.
    pub fn blend(&self) -> f32 {
        (self.since.elapsed().as_secs_f32() / BLEND_SECS).min(1.0)
    }

    pub fn current(&self) -> &Mood {
        &self.moods[self.current]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_studio_export_parses() {
        let moods = Moods::load();
        assert_eq!(moods.moods.len(), 8);
        assert_eq!(moods.current().name, "Watchful");
        for name in [
            "Curious",
            "Lonely",
            "Wrathful",
            "Delighted",
            "Suspicious",
            "Drowsy",
            "Blinded",
        ] {
            assert!(moods.moods.iter().any(|m| m.name == name), "missing {name}");
        }
    }

    #[test]
    fn hex_and_back() {
        assert_eq!(hex("#ff8000"), [1.0, 128.0 / 255.0, 0.0]);
        let back = from_linear(to_linear(0.5));
        assert!((back - 0.5).abs() < 1e-5);
    }

    #[test]
    fn a_blend_starts_at_the_old_mood() {
        let mut moods = Moods::load();
        let watchful = moods.now();
        moods.set("Wrathful");
        let start = moods.now();
        assert!(
            (start.heat - watchful.heat).abs() < 0.05,
            "starts where it was"
        );
        moods.since -= std::time::Duration::from_secs(2);
        assert_eq!(moods.now().heat, 1.2, "ends at Wrathful");
    }
}
