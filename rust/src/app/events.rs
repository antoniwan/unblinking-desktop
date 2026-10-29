//! winit's callbacks: window events, the frame loop and its pacing.

use std::time::Instant;

use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::WindowId;

use super::*;
use crate::camera_picker::Picked;
use crate::cli::Backend;
use crate::fullscreen;
use crate::gpu::Gpu;
use crate::mind::Mode;
use crate::prefs::Position;
use crate::senses::wake::Wake;
use crate::tray::Tray;
use crate::voice::Effect;
use crate::whisper::School;
use crate::{demo, pacing};

impl App {
    /// Draws a frame if one is due, and tells winit when to wake next.
    pub(super) fn pace(&mut self, event_loop: &ActiveEventLoop) {
        let hidden = self.presence == Presence::Hidden;
        // Look at faces less often while nobody can see the eye react.
        if let Some(eyes) = &self.eyes {
            eyes.set_resting(hidden);
        }
        let no_face_for = self
            .last_face
            .map_or(self.started.elapsed(), |t| t.elapsed());
        let p = pacing::Pacing {
            shown: self.presence.shown(),
            fading: matches!(self.presence, Presence::Opening(_) | Presence::Closing(..)),
            blind: self.prefs.blind,
            calm: self.mode == Some(Mode::Hunting)
                && no_face_for >= pacing::CALM_AFTER
                && self.last_noise.elapsed() >= pacing::CALM_AFTER,
            egui_open: self.panel.is_some() || self.picker.is_some(),
        };
        let now = Instant::now();
        let Some(interval) = pacing::frame_interval(p) else {
            // Nothing to draw. Take in the camera's reports so they don't
            // pile up in the channel, and tick slowly for the tray's sake.
            if let Some(eyes) = &mut self.eyes {
                eyes.look();
            }
            self.next_frame = now;
            event_loop.set_control_flow(ControlFlow::WaitUntil(now + pacing::HIDDEN_TICK));
            return;
        };
        if now >= self.next_frame {
            self.next_frame = pacing::next_due(self.next_frame, now, interval);
            if let Some(gpu) = &self.gpu {
                if hidden {
                    // A hidden window gets no redraw events from Windows,
                    // so hand ourselves one (it draws the egui windows only).
                    let id = gpu.window.id();
                    self.window_event(event_loop, id, WindowEvent::RedrawRequested);
                } else {
                    gpu.window.request_redraw();
                }
            }
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}

impl ApplicationHandler for App {
    /// Called once the event loop is ready for windows (on desktop: at start).
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        let order = match self.args.backend {
            Some(b) => vec![b],
            None => vec![Backend::Dx12, Backend::Vulkan],
        };
        // First look for a backend that can be see-through; if none can,
        // settle for an opaque eye rather than none at all.
        for allow_opaque in [false, true] {
            for &backend in &order {
                match Gpu::new(event_loop, backend, allow_opaque, &self.prefs) {
                    Ok(gpu) => {
                        let _ = gpu.window.set_cursor_hittest(!self.prefs.click_through);
                        self.gpu = Some(gpu);
                        // Present a transparent frame, then show the window:
                        // a window shown before its first frame flashes black.
                        self.appear();
                        // Without a tray the eye still works; it's just harder to reach.
                        self.tray = Tray::new(&self.prefs)
                            .map_err(|err| eprintln!("no tray icon: {err}"))
                            .ok();
                        if self.args.demo {
                            // The script plays the senses; the camera and mic stay off.
                            self.demo = Some(demo::Demo::new());
                        } else {
                            self.set_senses(!self.prefs.blind);
                        }
                        if let Some(gpu) = &self.gpu {
                            let window = gpu.window.clone();
                            self.refresh_monitors(&window);
                        }
                        if self.args.senses {
                            self.toggle_senses(event_loop);
                        }
                        if self.args.camera {
                            self.toggle_picker(event_loop);
                        }
                        return;
                    }
                    Err(err) => eprintln!("unblinking: {backend:?} failed: {err}"),
                }
            }
        }
        eprintln!("unblinking: no GPU backend worked");
        event_loop.exit();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        // Events for "Where's my camera?" go to the picker.
        if let (Some(picker), Some(gpu)) = (&mut self.picker, &self.gpu)
            && picker.id() == id
        {
            match event {
                WindowEvent::CloseRequested => self.picker = None,
                WindowEvent::RedrawRequested => {}
                other => picker.on_event(&gpu.device, &other),
            }
            return;
        }
        // Events for Blinki's Senses go to the panel.
        if let (Some(panel), Some(gpu)) = (&mut self.panel, &self.gpu)
            && panel.id() == id
        {
            match event {
                WindowEvent::CloseRequested => self.toggle_senses(event_loop),
                // It's drawn right after each frame of the eye instead.
                WindowEvent::RedrawRequested => {}
                other => panel.on_event(&gpu.device, &other),
            }
            return;
        }
        match event {
            WindowEvent::CloseRequested => self.close(AfterClose::Quit, event_loop),
            // Drag the eye anywhere with the left button.
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(gpu) = &self.gpu {
                    let _ = gpu.window.drag_window();
                }
            }
            // Right-click opens the same menu as the tray icon.
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => {
                if let (Some(gpu), Some(tray)) = (&self.gpu, &self.tray) {
                    tray.popup(&gpu.window);
                }
            }
            // Remember where it was dropped, once the dragging stops.
            WindowEvent::Moved(_) => self.save_move_at = Some(Instant::now() + SAVE_MOVE_AFTER),
            WindowEvent::RedrawRequested => {
                let snap_due = self.args.snap.is_some_and(|d| self.started.elapsed() >= d);
                // `take()` moves the Gpu out of the Option for a moment, so we
                // can borrow it and `self` at the same time; we put it back after.
                let Some(mut gpu) = self.gpu.take() else {
                    return;
                };
                let state = self.step(&gpu.window);
                if let Some(text) = self.pending_whisper.take() {
                    self.voice.effect(Effect::Chime);
                    gpu.whispers
                        .show(&text, School::of_mood(&self.moods.current().name));
                }
                if self.presence == Presence::Open
                    && let Some(text) = self.args.whisper.take()
                {
                    gpu.whispers
                        .show(&text, School::of_mood(&self.moods.current().name));
                }
                // Back from a full-screen app: one jealous line, once it's open.
                if self.presence == Presence::Open
                    && let Some(text) = self.fullscreen.take_line()
                    && self.prefs.whispers
                    && !self.prefs.blind
                {
                    gpu.whispers
                        .show(text, School::of_mood(&self.moods.current().name));
                    self.voice.say(text);
                }
                // While hidden (still drawing for the egui windows), skip the eye.
                if self.presence != Presence::Hidden {
                    gpu.render(&state);
                }
                // --record: keep this frame, as drawn.
                if self.recording.as_ref().is_some_and(|r| r.wants_frame()) {
                    let size = gpu.size();
                    let rgba = self.grab(&mut gpu, &state);
                    if let Some(r) = &mut self.recording {
                        r.add(size, rgba);
                    }
                }
                // --demo: when the performance is over, close the eye (and quit).
                if self.demo.as_ref().is_some_and(|d| d.done()) && self.presence == Presence::Open {
                    self.close(AfterClose::Quit, event_loop);
                }
                // The camera picker too, saving each move as it's made.
                if let Some(picker) = &mut self.picker {
                    let colors = self.moods.now().display_colors();
                    match picker.draw(
                        &gpu.device,
                        &gpu.queue,
                        &self.monitors,
                        self.prefs.camera.as_ref(),
                        &colors,
                    ) {
                        Picked::Nothing => {}
                        Picked::Moved(spot) => {
                            self.prefs.camera = spot;
                            self.prefs.save();
                        }
                        Picked::Close => self.picker = None,
                    }
                }
                // Blinki's Senses follows along, one picture per eye frame.
                if let (Some(panel), Some(snap)) = (&mut self.panel, &self.snapshot)
                    && let Some(sound) = panel.draw(&gpu.device, &gpu.queue, snap)
                {
                    self.prefs.sound = sound;
                    self.apply_sound();
                    self.prefs.save();
                    if let Some(tray) = &self.tray {
                        tray.refresh(&self.prefs, self.presence.shown());
                    }
                }
                if snap_due {
                    if let Err(err) = self.snap(&mut gpu, &state) {
                        eprintln!("snapshot failed: {err}");
                    }
                    event_loop.exit();
                }
                self.advance_presence(&mut gpu, event_loop);
                self.gpu = Some(gpu);
            }
            _ => {}
        }
    }

    /// After each batch of events: poll the tray and friends, then decide
    /// when the next frame is due and sleep until then (see pacing.rs).
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // `map_or_else(Vec::new, ..)`: no tray, no clicks.
        let actions = self.tray.as_ref().map_or_else(Vec::new, Tray::poll);
        for action in actions {
            self.act(action, event_loop);
        }
        let called = self.wake.as_ref().map_or_else(Vec::new, Wake::poll);
        for event in called {
            self.on_wake(event);
        }
        // Launched again: show the eye (as Electron's 'second-instance').
        if self.instance.as_ref().is_some_and(|i| i.woken()) && !self.presence.shown() {
            self.appear();
        }
        // Full screen: step aside (hiding, not quitting, and no pref saved),
        // then come back once it's over. The watcher itself asks Windows
        // only about once a second.
        match self
            .fullscreen
            .tick(self.prefs.hide_fullscreen, self.presence.shown())
        {
            Some(fullscreen::Do::Hide) => self.close(AfterClose::Hide, event_loop),
            Some(fullscreen::Do::Return) => self.appear(),
            None => {}
        }
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.state == HotKeyState::Pressed {
                self.toggle_senses(event_loop);
            }
        }
        if self.save_move_at.is_some_and(|t| Instant::now() >= t) {
            self.save_move_at = None;
            if let Some(gpu) = &self.gpu
                && let Ok(pos) = gpu.window.outer_position()
            {
                let logical = pos.to_logical::<f64>(gpu.window.scale_factor());
                self.prefs.position = Some(Position {
                    x: logical.x,
                    y: logical.y,
                });
                self.prefs.save();
            }
        }
        self.pace(event_loop);
    }
}
