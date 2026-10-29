//! Blinki's Senses: a window that shows what the eye perceives, styled as a
//! scrying glass rather than a debug console. Four lenses:
//!
//!   Sight    the live camera picture, every face box and landmark, the
//!            face it follows, and the calibrated centre
//!   Hearing  loudness over time, the baseline, the bang line, each flinch
//!   Mind     mode, mood (and why), the live numbers, the whisper log
//!   World    cursor, the eye's window, the screen, idle time, the clock
//!
//! Privacy: the picture and the loudness are only shown live. The camera
//! thread sends a small frame only while this window is open, and nothing
//! is ever written to disk. Blinding the eye blanks the Sight lens.

use std::collections::VecDeque;
use std::error::Error;
use std::sync::Arc;

use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Stroke, StrokeKind, Vec2 as EVec2};
use image::RgbImage;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use crate::desktop::{Point, Rect as DRect};
use crate::egui_window::EguiWindow;
use crate::mind::{BANG_LEVEL, Frame, Insight, Line, Mode, Vec2};
use crate::senses::Detection;
use crate::senses::expression::Expression;

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
    pub sound: crate::sound::Sound,
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
    ) -> Option<crate::sound::Sound> {
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

/// The mood's colours as egui colours, plus a few fixed inks.
pub(crate) struct Ink {
    pub deep: Color32,
    pub flame: Color32,
    pub core: Color32,
    pub glow: Color32,
    pub paper: Color32,
    pub panel: Color32,
    pub text: Color32,
    pub faint: Color32,
}

impl Ink {
    pub(crate) fn of(colors: &[[f32; 3]; 4]) -> Ink {
        let c = |[r, g, b]: [f32; 3]| {
            Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
        };
        Ink {
            deep: c(colors[0]),
            flame: c(colors[1]),
            core: c(colors[2]),
            glow: c(colors[3]),
            paper: Color32::from_rgb(0x0d, 0x0b, 0x10),
            panel: Color32::from_rgb(0x16, 0x13, 0x1b),
            text: Color32::from_rgb(0xec, 0xe6, 0xf2),
            faint: Color32::from_rgb(0x9a, 0x90, 0xa6),
        }
    }
}

pub(crate) fn with_alpha(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a * 255.0) as u8)
}

/// The whole window: a dark field with a slowly turning arcane circle, a
/// title, and four lenses.
fn scrying(
    ui: &mut egui::Ui,
    snap: &Snapshot,
    picture: Option<egui::TextureId>,
) -> Option<crate::sound::Sound> {
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
    sight_lens(&painter, inner(sight, "Sight"), snap, picture, &ink);
    let hearing = inner(hearing, "Hearing");
    let controls = Rect::from_min_max(
        Pos2::new(hearing.max.x - SOUND_WIDTH, hearing.min.y),
        hearing.max,
    );
    hearing_lens(
        &painter,
        Rect::from_min_max(hearing.min, Pos2::new(controls.min.x - 14.0, hearing.max.y)),
        snap,
        &ink,
    );
    let sound = sound_controls(ui, controls, snap.sound, &ink);
    mind_lens(&painter, inner(mind, "Mind"), snap, &ink);
    world_lens(&painter, inner(world, "World"), snap, &ink);
    sound
}

/// Faint concentric rings and rune ticks behind everything, turning slowly.
pub(crate) fn arcane_circle(p: &egui::Painter, full: Rect, ink: &Ink, t: f32) {
    let c = full.center();
    let r = full.width().min(full.height()) * 0.62;
    for (i, k) in [1.0f32, 0.82, 0.64].iter().enumerate() {
        p.circle_stroke(
            c,
            r * k,
            Stroke::new(1.0, with_alpha(ink.glow, 0.05 + 0.02 * i as f32)),
        );
    }
    let ticks = 48;
    for i in 0..ticks {
        let a = i as f32 / ticks as f32 * std::f32::consts::TAU + t * 0.02;
        let (s, co) = a.sin_cos();
        let long = if i % 4 == 0 { 0.06 } else { 0.025 };
        let from = c + EVec2::new(co, s) * r;
        let to = c + EVec2::new(co, s) * r * (1.0 - long);
        p.line_segment([from, to], Stroke::new(1.0, with_alpha(ink.glow, 0.08)));
    }
}

/// A lens frame: a dark glass with a glowing rim and a title. Returns the inside.
pub(crate) fn lens(p: &egui::Painter, r: Rect, title: &str, ink: &Ink) -> Rect {
    // A soft outer glow: a few wider, fainter rims.
    for (w, a) in [(7.0, 0.04), (4.0, 0.07), (2.0, 0.12)] {
        p.rect_stroke(
            r.expand(w * 0.5),
            CornerRadius::same(12),
            Stroke::new(w, with_alpha(ink.glow, a)),
            StrokeKind::Outside,
        );
    }
    p.rect_filled(r, CornerRadius::same(12), ink.panel);
    p.rect_stroke(
        r,
        CornerRadius::same(12),
        Stroke::new(1.0, with_alpha(ink.glow, 0.55)),
        StrokeKind::Inside,
    );
    diamond(p, r.left_top() + EVec2::new(19.0, 18.0), 4.5, ink.core);
    p.text(
        r.left_top() + EVec2::new(30.0, 10.0),
        Align2::LEFT_TOP,
        title.to_uppercase(),
        FontId::proportional(13.0),
        ink.core,
    );
    Rect::from_min_max(
        r.min + EVec2::new(14.0, 34.0),
        r.max - EVec2::new(14.0, 12.0),
    )
}

fn sight_lens(
    p: &egui::Painter,
    r: Rect,
    snap: &Snapshot,
    picture: Option<egui::TextureId>,
    ink: &Ink,
) {
    if snap.blind {
        centered_note(p, r, "Blinded. The camera is off.", ink);
        return;
    }
    let Some(tex) = picture else {
        let note = if snap.demo {
            "Demo: the camera stays off. The script plays the senses."
        } else if snap.can_see {
            "Opening the camera…"
        } else {
            "No camera"
        };
        centered_note(p, r, note, ink);
        return;
    };
    // Fit the picture into the lens without stretching. The picture is
    // mirrored for display, so it moves like a mirror (as you see yourself);
    // the labels and dots are mirrored with it.
    let aspect = snap
        .picture
        .as_ref()
        .map_or(16.0 / 9.0, |i| i.width() as f32 / i.height() as f32);
    let (w, h) = if r.width() / r.height() > aspect {
        (r.height() * aspect, r.height())
    } else {
        (r.width(), r.width() / aspect)
    };
    let img = Rect::from_center_size(r.center() - EVec2::new(0.0, 6.0), EVec2::new(w, h));
    p.image(
        tex,
        img,
        Rect::from_min_max(Pos2::new(1.0, 0.0), Pos2::new(0.0, 1.0)),
        Color32::WHITE,
    );
    p.rect_stroke(
        img,
        CornerRadius::same(6),
        Stroke::new(1.0, with_alpha(ink.glow, 0.4)),
        StrokeKind::Outside,
    );
    let at = |x: f32, y: f32| Pos2::new(img.max.x - x * img.width(), img.min.y + y * img.height());

    // The calibrated "sitting normally" point.
    if let Some([x, y]) = snap.rest_in_picture {
        let c = at(x, y);
        let s = Stroke::new(1.0, with_alpha(ink.text, 0.6));
        p.line_segment([c - EVec2::new(12.0, 0.0), c + EVec2::new(12.0, 0.0)], s);
        p.line_segment([c - EVec2::new(0.0, 12.0), c + EVec2::new(0.0, 12.0)], s);
        p.circle_stroke(c, 6.0, s);
        p.text(
            c + EVec2::new(10.0, 8.0),
            Align2::LEFT_TOP,
            "centre",
            FontId::proportional(11.0),
            with_alpha(ink.text, 0.6),
        );
    }

    let tracked = snap.insight.tracked;
    let biggest = snap
        .detections
        .iter()
        .map(|d| d.face.size)
        .fold(0.0, f64::max);
    for d in &snap.detections {
        let [bx, by, bw, bh] = d.bbox;
        let rect = Rect::from_two_pos(at(bx, by), at(bx + bw, by + bh));
        // Followed (nearest the tracked face), counted (big enough), or ignored.
        let is_tracked =
            tracked.is_some_and(|t| (t.x - d.face.x).abs() < 0.05 && (t.y - d.face.y).abs() < 0.05);
        let counted = d.face.score > 0.55 && d.face.size >= biggest * 0.8;
        let (color, width, label) = if is_tracked {
            (ink.core, 2.5, "watching")
        } else if counted {
            (ink.flame, 1.5, "company")
        } else {
            (with_alpha(ink.faint, 0.8), 1.0, "ignored")
        };
        p.rect_stroke(
            rect,
            CornerRadius::same(4),
            Stroke::new(width, color),
            StrokeKind::Outside,
        );
        p.text(
            rect.left_top() - EVec2::new(0.0, 3.0),
            Align2::LEFT_BOTTOM,
            format!(
                "{label} · {:.2} · {:.0}% wide",
                d.face.score,
                d.face.size * 100.0
            ),
            FontId::proportional(11.0),
            color,
        );
        for (i, [lx, ly]) in d.landmarks.iter().enumerate() {
            let dot = if i < 2 {
                ink.core
            } else {
                with_alpha(ink.flame, 0.9)
            };
            p.circle_filled(at(*lx, *ly), if i < 2 { 3.0 } else { 2.2 }, dot);
        }
        if is_tracked {
            // The point it aims at: between the eyes.
            let c = at(d.face.x as f32, d.face.y as f32);
            p.circle_stroke(c, 9.0, Stroke::new(1.5, ink.core));
        }
    }

    // Gaze dial in the corner: where it means to look (ring) and where the
    // eye is drawn looking (dot), in the shader's -1..1 gaze space.
    let dial_r = 34.0;
    let dc = Pos2::new(img.max.x - dial_r - 8.0, img.max.y - dial_r - 8.0);
    p.circle_filled(dc, dial_r, with_alpha(ink.paper, 0.75));
    p.circle_stroke(dc, dial_r, Stroke::new(1.0, with_alpha(ink.glow, 0.7)));
    p.line_segment(
        [dc - EVec2::new(dial_r, 0.0), dc + EVec2::new(dial_r, 0.0)],
        Stroke::new(0.5, with_alpha(ink.faint, 0.5)),
    );
    p.line_segment(
        [dc - EVec2::new(0.0, dial_r), dc + EVec2::new(0.0, dial_r)],
        Stroke::new(0.5, with_alpha(ink.faint, 0.5)),
    );
    let g = |v: Vec2| dc + EVec2::new(v.x as f32, -v.y as f32) * dial_r;
    p.circle_stroke(g(snap.insight.target), 5.0, Stroke::new(1.5, ink.flame));
    p.circle_filled(
        g(Vec2::new(
            snap.drawn_gaze[0] as f64,
            snap.drawn_gaze[1] as f64,
        )),
        3.5,
        ink.core,
    );
    p.text(
        dc - EVec2::new(0.0, dial_r + 4.0),
        Align2::CENTER_BOTTOM,
        "gaze",
        FontId::proportional(10.0),
        ink.faint,
    );
}

fn hearing_lens(p: &egui::Painter, r: Rect, snap: &Snapshot, ink: &Ink) {
    let Some(level) = snap.heard else {
        centered_note(
            p,
            r,
            if snap.blind {
                "Blinded. The mic is off."
            } else {
                "No microphone"
            },
            ink,
        );
        return;
    };
    let plot = r;
    let y = |v: f32| plot.max.y - v.clamp(0.0, 1.0) * plot.height();
    // Bang threshold and baseline.
    let bang = y(BANG_LEVEL as f32);
    p.line_segment(
        [Pos2::new(plot.min.x, bang), Pos2::new(plot.max.x, bang)],
        Stroke::new(1.0, with_alpha(ink.flame, 0.7)),
    );
    p.text(
        Pos2::new(plot.min.x + 2.0, bang - 2.0),
        Align2::LEFT_BOTTOM,
        "bang",
        FontId::proportional(10.0),
        ink.flame,
    );
    let base = y(snap.insight.baseline as f32);
    p.line_segment(
        [Pos2::new(plot.min.x, base), Pos2::new(plot.max.x, base)],
        Stroke::new(1.0, with_alpha(ink.faint, 0.5)),
    );
    // The loudness trail, newest on the right.
    let n = snap.levels.len().max(2);
    let x = |i: usize| plot.min.x + i as f32 / (LEVEL_HISTORY - 1) as f32 * plot.width();
    let start = LEVEL_HISTORY.saturating_sub(n);
    let pts: Vec<Pos2> = snap
        .levels
        .iter()
        .enumerate()
        .map(|(i, v)| Pos2::new(x(start + i), y(*v)))
        .collect();
    for w in pts.windows(2) {
        p.line_segment(
            [w[0], Pos2::new(w[0].x, plot.max.y)],
            Stroke::new(1.0, with_alpha(ink.deep, 0.35)),
        );
        p.line_segment([w[0], w[1]], Stroke::new(1.6, ink.core));
    }
    for &i in &snap.flinches {
        let fx = x(start + i);
        p.line_segment(
            [Pos2::new(fx, plot.min.y), Pos2::new(fx, plot.max.y)],
            Stroke::new(2.0, Color32::from_rgb(255, 60, 40)),
        );
        p.text(
            Pos2::new(fx + 3.0, plot.min.y),
            Align2::LEFT_TOP,
            "flinch",
            FontId::proportional(10.0),
            Color32::from_rgb(255, 90, 60),
        );
    }
    // The numbers, over the trail's top right.
    p.text(
        plot.right_top(),
        Align2::RIGHT_TOP,
        format!("loudness {level:.2} · used to {:.2}", snap.insight.baseline),
        FontId::proportional(11.0),
        ink.core,
    );
}

fn mind_lens(p: &egui::Painter, r: Rect, snap: &Snapshot, ink: &Ink) {
    let f = &snap.frame;
    let mut y = r.min.y;
    let mode = match f.mode {
        Mode::Locked => "Locked on",
        Mode::Hunting => "Hunting",
        Mode::Blind => "Blind",
    };
    p.text(
        Pos2::new(r.min.x, y),
        Align2::LEFT_TOP,
        mode,
        FontId::proportional(22.0),
        ink.text,
    );
    p.text(
        Pos2::new(r.max.x, y + 4.0),
        Align2::RIGHT_TOP,
        format!("awake {:.0}s", snap.insight.t),
        FontId::monospace(12.0),
        ink.faint,
    );
    y += 34.0;

    // Mood: swatches, name, blend, and why.
    for (i, c) in [ink.deep, ink.flame, ink.core, ink.glow].iter().enumerate() {
        p.circle_filled(Pos2::new(r.min.x + 8.0 + i as f32 * 20.0, y + 9.0), 7.0, *c);
    }
    p.text(
        Pos2::new(r.min.x + 92.0, y),
        Align2::LEFT_TOP,
        &snap.mood,
        FontId::proportional(18.0),
        ink.flame,
    );
    y += 26.0;
    bar(
        p,
        Rect::from_min_size(Pos2::new(r.min.x, y), EVec2::new(r.width(), 5.0)),
        snap.mood_blend,
        ink.glow,
        ink,
    );
    y += 10.0;
    p.text(
        Pos2::new(r.min.x, y),
        Align2::LEFT_TOP,
        format!("because: {}", snap.mood_reason),
        FontId::proportional(12.0),
        ink.faint,
    );
    y += 18.0;

    // What it reads in your face, and what it remembers of you, in words.
    if let Some(e) = &snap.expression {
        let face = if e.confidence < 0.5 {
            "hard to read"
        } else if e.facing < 0.35 {
            "turned away"
        } else if e.smile > 0.6 {
            "smiling"
        } else if e.surprise > 0.6 {
            "surprised"
        } else {
            "calm"
        };
        p.text(
            Pos2::new(r.min.x, y),
            Align2::LEFT_TOP,
            format!("your face: {face}"),
            FontId::proportional(12.0),
            ink.faint,
        );
        y += 16.0;
    }
    if let Some((traits, first)) = &snap.remembers {
        let known = if traits.is_empty() {
            "still getting to know you".to_string()
        } else {
            format!("knows you as {}", traits.join(", "))
        };
        p.text(
            Pos2::new(r.min.x, y),
            Align2::LEFT_TOP,
            known,
            FontId::proportional(12.0),
            ink.faint,
        );
        y += 16.0;
        if let Some(line) = first {
            p.text(
                Pos2::new(r.min.x, y),
                Align2::LEFT_TOP,
                format!("remembers: “{line}”"),
                FontId::proportional(12.0),
                ink.faint,
            );
            y += 16.0;
        }
    }
    y += 8.0;

    // The live numbers.
    let rows = [
        ("heat", f.heat as f32, 1.8),
        ("flare", f.flare as f32, 1.0),
        ("wake", f.wide as f32, 1.2),
        ("pupil", f.pupil as f32, 1.0),
        ("dim", f.dim as f32, 1.0),
        ("shake", (f.shake.x.hypot(f.shake.y) * 80.0) as f32, 1.0),
    ];
    for (name, v, max) in rows {
        p.text(
            Pos2::new(r.min.x, y),
            Align2::LEFT_TOP,
            name,
            FontId::proportional(12.0),
            ink.faint,
        );
        bar(
            p,
            Rect::from_min_size(
                Pos2::new(r.min.x + 56.0, y + 4.0),
                EVec2::new(r.width() - 110.0, 7.0),
            ),
            v / max,
            ink.flame,
            ink,
        );
        p.text(
            Pos2::new(r.max.x, y),
            Align2::RIGHT_TOP,
            format!("{v:.2}"),
            FontId::monospace(12.0),
            ink.text,
        );
        y += 19.0;
    }
    y += 8.0;

    // Whispers: the latest first, and what's still cooling down.
    p.text(
        Pos2::new(r.min.x, y),
        Align2::LEFT_TOP,
        "whispers",
        FontId::proportional(12.0),
        ink.core,
    );
    let t = snap.insight.t;
    let hush = (snap.insight.hush_until - t).max(0.0);
    if hush > 0.0 {
        p.text(
            Pos2::new(r.max.x, y),
            Align2::RIGHT_TOP,
            format!("hushed {hush:.0}s"),
            FontId::monospace(11.0),
            ink.faint,
        );
    }
    y += 18.0;
    for (at, kind, text) in snap.whispers.iter().rev().take(4) {
        p.text(
            Pos2::new(r.min.x, y),
            Align2::LEFT_TOP,
            format!("“{text}”"),
            FontId::proportional(13.0),
            ink.text,
        );
        p.text(
            Pos2::new(r.max.x, y),
            Align2::RIGHT_TOP,
            format!("{kind:?} · {:.0}s ago", t - at),
            FontId::monospace(11.0),
            ink.faint,
        );
        y += 17.0;
    }
    let cooling: Vec<String> = snap
        .insight
        .cooldowns
        .iter()
        .filter(|(_, until)| *until > t)
        .map(|(k, until)| format!("{k:?} {:.0}s", until - t))
        .collect();
    if !cooling.is_empty() && y < r.max.y - 14.0 {
        p.text(
            Pos2::new(r.min.x, y + 2.0),
            Align2::LEFT_TOP,
            format!("cooling: {}", cooling.join(", ")),
            FontId::proportional(11.0),
            ink.faint,
        );
    }
}

fn world_lens(p: &egui::Painter, r: Rect, snap: &Snapshot, ink: &Ink) {
    let pt = |o: Option<Point>| o.map_or("unknown".into(), |p| format!("{:.0}, {:.0}", p.x, p.y));
    let rows = [
        ("cursor", pt(snap.cursor)),
        ("eye", pt(snap.eye)),
        (
            "screen",
            snap.display.map_or("unknown".into(), |d| {
                format!("{:.0} × {:.0} at {:.0}, {:.0}", d.width, d.height, d.x, d.y)
            }),
        ),
        (
            "idle",
            snap.idle.map_or("unknown".into(), |s| {
                format!("{s:.0} s since your last input")
            }),
        ),
        ("clock", format!("{:02}:{:02}", snap.clock.0, snap.clock.1)),
        ("camera", snap.camera.clone()),
        ("full screen", "not watched yet".into()),
    ];
    let mut y = r.min.y;
    for (k, v) in rows {
        p.text(
            Pos2::new(r.min.x, y),
            Align2::LEFT_TOP,
            k,
            FontId::proportional(12.0),
            ink.faint,
        );
        p.text(
            Pos2::new(r.min.x + 90.0, y),
            Align2::LEFT_TOP,
            v,
            FontId::monospace(12.0),
            ink.text,
        );
        y += 19.0;
    }
}

/// A four-pointed star, the panel's mark.
pub(crate) fn star(p: &egui::Painter, c: Pos2, r: f32, color: Color32) {
    let pts: Vec<Pos2> = (0..8)
        .map(|i| {
            let a = i as f32 * std::f32::consts::FRAC_PI_4 - std::f32::consts::FRAC_PI_2;
            let len = if i % 2 == 0 { r } else { r * 0.32 };
            c + EVec2::new(a.cos(), a.sin()) * len
        })
        .collect();
    // A star isn't convex, so draw it as four thin triangles from the centre.
    for i in 0..8 {
        let (a, b) = (pts[i], pts[(i + 1) % 8]);
        p.add(egui::Shape::convex_polygon(
            vec![c, a, b],
            color,
            Stroke::NONE,
        ));
    }
}

/// A small diamond marking each lens title.
pub(crate) fn diamond(p: &egui::Painter, c: Pos2, r: f32, color: Color32) {
    let pts = vec![
        c + EVec2::new(0.0, -r),
        c + EVec2::new(r, 0.0),
        c + EVec2::new(0.0, r),
        c + EVec2::new(-r, 0.0),
    ];
    p.add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
}

fn bar(p: &egui::Painter, r: Rect, v: f32, color: Color32, ink: &Ink) {
    p.rect_filled(r, CornerRadius::same(3), with_alpha(ink.text, 0.06));
    let fill = Rect::from_min_size(r.min, EVec2::new(r.width() * v.clamp(0.0, 1.0), r.height()));
    p.rect_filled(fill, CornerRadius::same(3), color);
}

pub(crate) fn centered_note(p: &egui::Painter, r: Rect, text: &str, ink: &Ink) {
    p.text(
        r.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(15.0),
        ink.faint,
    );
}

/// How wide the Sound controls are, at the right of the Hearing lens.
const SOUND_WIDTH: f32 = 190.0;

/// Sound on/off, then Music, Voice and Effects, each with a switch and a
/// volume. Returns the new mixer if anything changed.
fn sound_controls(
    ui: &mut egui::Ui,
    r: Rect,
    mut sound: crate::sound::Sound,
    ink: &Ink,
) -> Option<crate::sound::Sound> {
    let before = sound;
    ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
        let v = ui.visuals_mut();
        v.selection.bg_fill = ink.flame;
        v.widgets.inactive.bg_fill = with_alpha(ink.text, 0.08);
        v.widgets.hovered.bg_fill = with_alpha(ink.glow, 0.3);
        v.override_text_color = Some(ink.text);
        ui.spacing_mut().slider_width = r.width() - 8.0;
        ui.checkbox(
            &mut sound.master,
            egui::RichText::new("Sound").color(ink.core),
        );
        ui.add_space(2.0);
        let row = |ui: &mut egui::Ui, on: &mut bool, volume: &mut f32, label: &str| {
            ui.add_enabled_ui(sound.master, |ui| {
                ui.checkbox(on, label);
                ui.add_enabled(*on, egui::Slider::new(volume, 0.0..=1.0).show_value(false));
            });
        };
        row(ui, &mut sound.music, &mut sound.music_volume, "Music");
        row(ui, &mut sound.voice, &mut sound.voice_volume, "Voice");
        row(ui, &mut sound.fx, &mut sound.fx_volume, "Effects");
    });
    (sound != before).then_some(sound)
}
