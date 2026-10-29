//! The sound mixer's settings: three channels, each with a mute and a
//! volume, under one master switch.
//!
//!   Music   the drone under the fire
//!   Voice   the whispers, spoken aloud
//!   FX      sound effects: the flinch at a bang, the chime of a whisper

use serde::{Deserialize, Serialize};

/// `#[serde(default)]` fills any key a saved file lacks from `Default`,
/// so older or hand-edited prefs still load.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Sound {
    pub master: bool,
    pub music: bool,
    pub voice: bool,
    pub fx: bool,
    pub music_volume: f32,
    pub voice_volume: f32,
    pub fx_volume: f32,
}

/// Silent until you ask, like the Electron app's Voice (off by default).
impl Default for Sound {
    fn default() -> Self {
        Sound {
            master: true,
            music: false,
            voice: false,
            fx: false,
            music_volume: 1.0,
            voice_volume: 1.0,
            fx_volume: 1.0,
        }
    }
}

/// How loud each channel really plays, 0 (silent) to 1, after the mutes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Levels {
    pub music: f32,
    pub voice: f32,
    pub fx: f32,
}

impl Sound {
    /// The old single "Voice" toggle (drone and speech together) becomes
    /// all three channels on or off.
    pub fn from_old_voice(on: bool) -> Sound {
        Sound {
            music: on,
            voice: on,
            fx: on,
            ..Sound::default()
        }
    }

    pub fn levels(&self) -> Levels {
        let gate = |on: bool, volume: f32| {
            if self.master && on {
                volume.clamp(0.0, 1.0)
            } else {
                0.0
            }
        };
        Levels {
            music: gate(self.music, self.music_volume),
            voice: gate(self.voice, self.voice_volume),
            fx: gate(self.fx, self.fx_volume),
        }
    }

    /// Whether anything at all should play; Electron's "voice" key mirrors this.
    pub fn audible(&self) -> bool {
        let l = self.levels();
        l.music > 0.0 || l.voice > 0.0 || l.fx > 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_on() -> Sound {
        Sound {
            music: true,
            voice: true,
            fx: true,
            ..Sound::default()
        }
    }

    #[test]
    fn master_silences_everything() {
        let s = Sound {
            master: false,
            ..all_on()
        };
        assert_eq!(s.levels(), Levels::default());
        assert!(!s.audible());
    }

    #[test]
    fn each_channel_mutes_alone() {
        let s = Sound {
            voice: false,
            ..all_on()
        };
        let l = s.levels();
        assert_eq!((l.music, l.voice, l.fx), (1.0, 0.0, 1.0));
        let s = Sound {
            fx: false,
            music: false,
            ..all_on()
        };
        let l = s.levels();
        assert_eq!((l.music, l.voice, l.fx), (0.0, 1.0, 0.0));
    }

    #[test]
    fn volumes_scale_and_are_clamped() {
        let s = Sound {
            music_volume: 0.25,
            fx_volume: 3.0,
            voice_volume: -1.0,
            ..all_on()
        };
        let l = s.levels();
        assert_eq!((l.music, l.voice, l.fx), (0.25, 0.0, 1.0));
    }

    #[test]
    fn the_old_voice_toggle_carries_over() {
        assert_eq!(
            Sound::from_old_voice(true).levels(),
            Levels {
                music: 1.0,
                voice: 1.0,
                fx: 1.0
            }
        );
        assert!(!Sound::from_old_voice(false).audible());
    }

    #[test]
    fn a_partial_saved_mixer_keeps_defaults_for_the_rest() {
        let s: Sound = serde_json::from_str(r#"{ "music": true, "musicVolume": 0.5 }"#).unwrap();
        assert_eq!(
            s,
            Sound {
                music: true,
                music_volume: 0.5,
                ..Sound::default()
            }
        );
    }
}
