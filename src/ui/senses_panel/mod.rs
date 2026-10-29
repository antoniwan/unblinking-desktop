//! Blinki's Senses: a window that shows what the eye perceives, styled as a
//! scrying glass rather than a debug console. Four lenses:
//!
//!   Sight    the live camera picture, every face box and landmark, the
//!            face it follows, and the calibrated centre
//!   Hearing  loudness over time, the baseline, the bang line, each flinch
//!   Mind     mode, mood (and why), the live numbers, the whisper log
//!   World    cursor, the eye's window, the screen, idle time, the clock
//!
//! Each lens has a file of its own (`sight`, `hearing`, `mind`, `world`);
//! the inks, the lens frame and the little drawn marks they share live in
//! `style`. This file holds the window itself and lays the lenses out.
//!
//! Privacy: the picture and the loudness are only shown live. The camera
//! thread sends a small frame only while this window is open, and nothing
//! is ever written to disk. Blinding the eye blanks the Sight lens.

// `mod x;` pulls in `x.rs` from this folder. These are private: only this
// module draws the lenses. `style` is `pub(crate)` because "Where's my
// camera?" borrows its inks and marks.
mod hearing;
mod mind;
mod sight;
pub(crate) mod style;
mod world;

use std::collections::VecDeque;
use std::error::Error;
use std::sync::Arc;

use egui::{Align2, FontId, Pos2, Rect, Vec2 as EVec2};
use image::RgbImage;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use crate::mind::{Frame, Insight, Line};
use crate::platform::desktop::{Point, Rect as DRect};
use crate::senses::Detection;
use crate::senses::expression::Expression;
use crate::ui::egui_window::EguiWindow;
use style::{Ink, arcane_circle, lens, star};

/// How many loudness samples the Hearing lens keeps (about 8 s at 60 fps).
pub const LEVEL_HISTORY: usize = 480;

/// Everything the panel shows for one frame, gathered by the app.
pub struct Snapshot {
    pub frame: Frame,
    pub insight: Insight,
    /// The gaze the shader actually got, after the opening/closing easing.
    pub drawn_gaze: [f32; 2],
    pub mood: String,
    pub mood_reason: &'static str,
    pub mood_blend: f32,
    /// Display colours of the current look: deep, flame, core, glow.
    pub colors: [[f32; 3]; 4],
    pub detections: Vec<Detection>,
    pub can_see: bool,
    pub blind: bool,
    /// Playing --demo: no camera, no mic.
    pub demo: bool,
    pub picture: Option<Arc<RgbImage>>,
    /// Where "sitting normally" lands in the picture, 0..1, if it can be placed.
    pub rest_in_picture: Option<[f32; 2]>,
    pub levels: VecDeque<f32>,
    /// Indexes into `levels` where a flinch fired.
    pub flinches: Vec<usize>,
    pub heard: Option<f64>,
    pub whispers: Vec<(f64, Line, &'static str)>,
    pub cursor: Option<Point>,
    pub eye: Option<Point>,
    pub display: Option<DRect>,
    pub idle: Option<f64>,
    pub clock: (u32, u32),
    /// Where the webcam is set to be, in words.
    pub camera: String,
    /// What it remembers of you, while "Remember our days" is on: its
    /// traits for you, in words, and the newest first in its journal.
    /// Never numbers: Remembrance is felt, not scored.
    pub remembers: Option<(Vec<&'static str>, Option<String>)>,
    /// The reading of your face, while "Read my face" is on.
    pub expression: Option<Expression>,
    /// The mixer, for the Sound controls.
    pub sound: crate::audio::sound::Sound,
}

pub struct SensesPanel {
    win: EguiWindow,
    picture: Option<(Arc<RgbImage>, egui::TextureHandle)>,
}

impl SensesPanel {
    pub fn new(
        event_loop: &ActiveEventLoop,
        gpu: (&wgpu::Instance, &wgpu::Adapter, &wgpu::Device),
    ) -> Result<SensesPanel, Box<dyn Error>> {
        let win = EguiWindow::new(
            event_loop,
            gpu,
            "Blinki's Senses",
            (1080.0, 720.0),
            (760.0, 520.0),
        )?;
        Ok(SensesPanel { win, picture: None })
    }

    pub fn id(&self) -> WindowId {
        self.win.id()
    }

    pub fn on_event(&mut self, device: &wgpu::Device, event: &winit::event::WindowEvent) {
        self.win.on_event(device, event);
    }

    /// Draws one frame. Returns the mixer if its controls were changed.
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        snap: &Snapshot,
    ) -> Option<crate::audio::sound::Sound> {
        // A new picture replaces the texture's pixels; the same Arc means no change.
        if let Some(pic) = &snap.picture {
            let fresh = self
                .picture
                .as_ref()
                .is_none_or(|(old, _)| !Arc::ptr_eq(old, pic));
            if fresh {
                let image = egui::ColorImage::from_rgb(
                    [pic.width() as usize, pic.height() as usize],
                    pic.as_raw(),
                );
                match &mut self.picture {
                    Some((old, tex)) => {
                        tex.set(image, egui::TextureOptions::LINEAR);
                        *old = pic.clone();
                    }
                    None => {
                        let tex =
                            self.win
                                .ctx
                                .load_texture("sight", image, egui::TextureOptions::LINEAR);
                        self.picture = Some((pic.clone(), tex));
                    }
                }
            }
        } else {
            // Blind, or no camera: let the last picture go.
            self.picture = None;
        }
        let picture = self.picture.as_ref().map(|(_, t)| t.id());
        let mut changed = None;
        self.win
            .draw(device, queue, |ui| changed = scrying(ui, snap, picture));
        changed
    }
}

// ---------------------------------------------------------------- drawing --

/// The whole window: a dark field with a slowly turning arcane circle, a
/// title, and four lenses.
fn scrying(
    ui: &mut egui::Ui,
    snap: &Snapshot,
    picture: Option<egui::TextureId>,
) -> Option<crate::audio::sound::Sound> {
    let ink = Ink::of(&snap.colors);
    let full = ui.max_rect();
    let painter = ui.painter().clone();
    painter.rect_filled(full, 0.0, ink.paper);
    arcane_circle(&painter, full, &ink, snap.insight.t as f32);

    let margin = 18.0;
    let area = full.shrink(margin);
    // Title.
    star(
        &painter,
        area.left_top() + EVec2::new(10.0, 15.0),
        10.0,
        ink.flame,
    );
    painter.text(
        area.left_top() + EVec2::new(28.0, 0.0),
        Align2::LEFT_TOP,
        "Blinki's Senses",
        FontId::proportional(26.0),
        ink.flame,
    );
    painter.text(
        area.left_top() + EVec2::new(4.0, 32.0),
        Align2::LEFT_TOP,
        "what the eye perceives, live. Nothing here is recorded.",
        FontId::proportional(13.0),
        ink.faint,
    );
    painter.text(
        area.right_top(),
        Align2::RIGHT_TOP,
        format!("{:02}:{:02}", snap.clock.0, snap.clock.1),
        FontId::monospace(18.0),
        ink.faint,
    );

    let body = Rect::from_min_max(area.left_top() + EVec2::new(0.0, 58.0), area.right_bottom());
    let gap = 14.0;
    let left_w = (body.width() - gap) * 0.56;
    let left = Rect::from_min_size(body.min, EVec2::new(left_w, body.height()));
    let right = Rect::from_min_max(Pos2::new(left.max.x + gap, body.min.y), body.max);
    let sight_h = (left.height() - gap) * 0.66;
    let sight = Rect::from_min_size(left.min, EVec2::new(left.width(), sight_h));
    let hearing = Rect::from_min_max(Pos2::new(left.min.x, sight.max.y + gap), left.max);
    let mind_h = (right.height() - gap) * 0.68;
    let mind = Rect::from_min_size(right.min, EVec2::new(right.width(), mind_h));
    let world = Rect::from_min_max(Pos2::new(right.min.x, mind.max.y + gap), right.max);

    let inner = |r: Rect, title: &str| lens(&painter, r, title, &ink);
    sight::sight_lens(&painter, inner(sight, "Sight"), snap, picture, &ink);
    let hearing = inner(hearing, "Hearing");
    let controls = Rect::from_min_max(
        Pos2::new(hearing.max.x - hearing::SOUND_WIDTH, hearing.min.y),
        hearing.max,
    );
    hearing::hearing_lens(
        &painter,
        Rect::from_min_max(hearing.min, Pos2::new(controls.min.x - 14.0, hearing.max.y)),
        snap,
        &ink,
    );
    let sound = hearing::sound_controls(ui, controls, snap.sound, &ink);
    mind::mind_lens(&painter, inner(mind, "Mind"), snap, &ink);
    world::world_lens(&painter, inner(world, "World"), snap, &ink);
    sound
}
