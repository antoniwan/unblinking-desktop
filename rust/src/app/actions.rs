//! What the tray, the hotkey and the wake word ask for: menu actions, the
//! extra windows, and opening or closing the senses.

use winit::event_loop::ActiveEventLoop;

use super::*;
use crate::camera_picker::CameraPicker;
use crate::mind::{self};
use crate::prefs::Position;
use crate::remembrance::{self, Remembrance};
use crate::senses::Detector;
use crate::senses::camera::Eyes;
use crate::senses::face::YuNet;
use crate::senses::mic::Ears;
use crate::senses::wake::{Wake, WakeEvent};
use crate::senses_panel::SensesPanel;
use crate::session;
use crate::tray::Action;
use crate::wellbeing::Watch;

impl App {
    /// Opens "Where's my camera?", or closes it.
    pub(super) fn toggle_picker(&mut self, event_loop: &ActiveEventLoop) {
        if self.picker.take().is_some() {
            return;
        }
        let Some(gpu) = &self.gpu else { return };
        let window = gpu.window.clone();
        match CameraPicker::new(event_loop, (&gpu.instance, &gpu.adapter, &gpu.device)) {
            Ok(picker) => self.picker = Some(picker),
            Err(err) => eprintln!("could not open Where's my camera?: {err}"),
        }
        self.refresh_monitors(&window);
    }

    /// Opens Blinki's Senses, or closes it (which also stops the camera preview).
    pub(super) fn toggle_senses(&mut self, event_loop: &ActiveEventLoop) {
        if self.panel.take().is_some() {
            self.snapshot = None;
            if let Some(eyes) = &mut self.eyes {
                eyes.set_preview(false);
            }
            return;
        }
        let Some(gpu) = &self.gpu else { return };
        match SensesPanel::new(event_loop, (&gpu.instance, &gpu.adapter, &gpu.device)) {
            Ok(panel) => {
                self.panel = Some(panel);
                if let Some(eyes) = &mut self.eyes {
                    eyes.set_preview(true);
                }
            }
            Err(err) => eprintln!("could not open Blinki's Senses: {err}"),
        }
    }

    /// Opens or closes the camera and mic together. Closing really stops the
    /// devices: setting the Options to `None` drops the Eyes and Ears, and
    /// their `Drop` code lets go of the hardware.
    pub(super) fn set_senses(&mut self, open: bool) {
        if !open {
            self.eyes = None;
            self.ears = None;
            self.expression = None;
            self.sync_wake();
            eprintln!("senses closed");
            return;
        }
        if self.eyes.is_none() {
            self.eyes = Some(Eyes::open(|| {
                let net = YuNet::new().map_err(|e| e.to_string())?;
                Ok(Box::new(net) as Box<dyn Detector>)
            }));
            if let Some(eyes) = &mut self.eyes {
                eyes.set_feel(self.prefs.feel);
            }
        }
        if self.ears.is_none() {
            // `.map_err(..).ok()` logs a failure and carries on deaf:
            // a missing sense never stops the eye.
            self.ears = Ears::open()
                .map_err(|err| eprintln!("no hearing: {err}"))
                .ok();
        }
        self.sync_wake();
    }

    /// Starts or stops the wake word to match its toggle. Blind wins: a
    /// blinded eye hears nothing, names included, and dropping the Wake
    /// frees the mic.
    pub(super) fn sync_wake(&mut self) {
        if !self.prefs.wake_word || self.prefs.blind {
            self.wake = None;
        } else if self.wake.is_none() {
            // No name pref yet, so only "Hey, OG Blinki" and "Hey Blinki".
            self.wake = Some(Wake::start(self.prefs.name.clone()));
        }
    }

    /// Someone called it, or the wake word could not work here.
    pub(super) fn on_wake(&mut self, event: WakeEvent) {
        match event {
            WakeEvent::Heard(words) => {
                eprintln!("wake word: heard {words:?}");
                if !self.presence.shown() {
                    self.appear();
                }
                self.mind.called();
            }
            WakeEvent::Failed(why) => {
                // Said once: the toggle turns itself off, so this can't repeat
                // until the user turns it back on.
                eprintln!("wake word off: {why}");
                self.wake = None;
                self.prefs.wake_word = false;
                self.prefs.save();
                if self.prefs.whispers {
                    const DEAF: &str = "I can't hear names on this computer yet.";
                    self.pending_whisper = Some(DEAF.to_string());
                    self.voice.say(DEAF);
                }
            }
        }
        if let Some(tray) = &self.tray {
            tray.refresh(&self.prefs, self.presence.shown());
        }
    }

    /// Carries out one menu or tray click.
    pub(super) fn act(&mut self, action: Action, event_loop: &ActiveEventLoop) {
        let Some(gpu) = &self.gpu else { return };
        let window = gpu.window.clone();
        // A match arm can have a guard (`if ...`); arms are tried in order.
        match action {
            Action::Quit => self.close(AfterClose::Quit, event_loop),
            Action::Senses => self.toggle_senses(event_loop),
            Action::WhereCamera => self.toggle_picker(event_loop),
            Action::StartWithComputer(on) => {
                if let Err(err) = session::set_starts_with_computer(on) {
                    eprintln!("could not change start with the computer: {err}");
                }
            }
            Action::ToggleShown if self.presence.shown() => {
                self.close(AfterClose::Hide, event_loop)
            }
            Action::ToggleShown => self.appear(),
            Action::Blind(on) => {
                self.prefs.blind = on;
                self.set_senses(!on);
            }
            Action::Whispers(on) => self.prefs.whispers = on,
            Action::HideFullscreen(on) => self.prefs.hide_fullscreen = on,
            Action::WakeWord(on) => {
                self.prefs.wake_word = on;
                self.sync_wake();
            }
            Action::Mute(channel, on) => {
                channel.set(&mut self.prefs.sound, on);
                self.apply_sound();
            }
            Action::LookAtMe => {
                // The face the camera sees now, wherever it is, becomes "sitting normally".
                let face = self
                    .eyes
                    .as_mut()
                    .and_then(|e| e.look().detections.first().map(|d| d.face));
                match face {
                    Some(face) => {
                        let [x, y, _] = mind::face_in_room(&face);
                        let rest = mind::REST_SEAT;
                        self.prefs.look_offset = Some(Position {
                            x: x - rest.x,
                            y: y - rest.y,
                        });
                        eprintln!("look at me: offset {:?}", self.prefs.look_offset);
                    }
                    None => eprintln!("look at me: no face in view"),
                }
            }
            Action::ClickThrough(on) => {
                self.prefs.click_through = on;
                let _ = window.set_cursor_hittest(!on);
            }
            Action::Theme(id) => {
                self.prefs.theme = Some(id.to_string());
                self.moods.set_theme(id);
            }
            Action::ReadFace(on) => {
                self.prefs.feel = on;
                if let Some(eyes) = &mut self.eyes {
                    eyes.set_feel(on);
                }
                if !on {
                    self.expression = None;
                }
            }
            Action::Forget => {
                // Wipes the file. If it's still remembering, it starts afresh.
                let now = remembrance::unix_now();
                match &mut self.book {
                    Some(book) => book.forget(now),
                    None => Remembrance::load(now).forget(now),
                }
                self.watch = Watch::new();
                eprintln!("forgot our days");
            }
            Action::Remember(on) => {
                self.prefs.remember = on;
                // Off: save what it has and stop noticing. The file stays
                // on this computer until "Forget our days".
                if let Some(mut book) = self.book.take() {
                    book.save(remembrance::unix_now());
                }
                if on {
                    self.book = Some(Remembrance::load(remembrance::unix_now()));
                    self.watch = Watch::new();
                }
            }
        }
        if !matches!(
            action,
            Action::ToggleShown
                | Action::Quit
                | Action::Senses
                | Action::WhereCamera
                | Action::StartWithComputer(_)
                | Action::Forget
        ) {
            self.prefs.save();
        }
        if let Some(tray) = &self.tray {
            tray.refresh(&self.prefs, self.presence.shown());
        }
    }

    /// Hands the mixer to the voice, and keeps Electron's "voice" key in step.
    pub(super) fn apply_sound(&mut self) {
        self.voice.apply(self.prefs.sound.levels());
        self.prefs.voice = self.prefs.sound.audible();
    }
}
