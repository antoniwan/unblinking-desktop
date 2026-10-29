//! Whispers on screen: the line the eye just said, written under it letter
//! by letter. Each mood belongs to a school of magic, and each school has
//! its own entrance, ported from the Blinki Studio's CSS animations:
//!
//!   Pyromancy  rises out of the heat         Arcane    turns in like a rune
//!   Umbral     condenses out of smoke        Storm     crackles on
//!   Fae        gathers from scattered sparks Druidic   grows up from the ground
//!   Astral     fades in like starlight       Warding   is etched left to right
//!
//! Drawn with egui into the eye's own window, after the eye, so it shares
//! the transparent surface and its premultiplied colours.

use std::time::Instant;

use egui::{Color32, FontId, Pos2, Vec2 as EVec2};

/// How long a whisper stays (as in the Electron app: 3.2 s, then a 0.9 s fade).
const HOLD_SECS: f32 = 3.2;
const FADE_SECS: f32 = 0.9;
/// The gap between one letter's entrance and the next.
const STAGGER_SECS: f32 = 0.035;
const FONT_SIZE: f32 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum School {
    Pyromancy,
    Arcane,
    Umbral,
    Storm,
    Fae,
    Druidic,
    Astral,
    Warding,
}

impl School {
    /// The studio's pairing of moods and schools (see the README on main).
    pub fn of_mood(name: &str) -> School {
        match name {
            "Curious" => School::Arcane,
            "Lonely" => School::Umbral,
            "Wrathful" => School::Storm,
            "Delighted" => School::Fae,
            "Suspicious" => School::Druidic,
            "Drowsy" => School::Astral,
            "Blinded" | "Worried" => School::Warding,
            _ => School::Pyromancy,
        }
    }

    /// How long one letter's entrance takes.
    fn entrance(self) -> f32 {
        match self {
            School::Pyromancy => 0.7,
            School::Arcane => 0.9,
            School::Umbral => 1.6,
            School::Storm => 0.35,
            School::Fae => 1.0,
            School::Druidic => 0.8,
            School::Astral => 2.0,
            School::Warding => 0.6,
        }
    }
}

/// How one letter looks at a moment of its entrance: how visible, how far
/// from its place, how big, turned how far, how blurred, how bright.
struct Pose {
    alpha: f32,
    offset: EVec2,
    scale: f32,
    angle: f32,
    blur: f32,
    glow: f32,
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// The letter's pose `t` seconds into its own entrance. `seed` varies the
/// scattered schools from letter to letter.
fn pose(school: School, t: f32, seed: f32) -> Pose {
    let k = (t / school.entrance()).clamp(0.0, 1.0);
    let e = ease_out(k);
    let mut p = Pose {
        alpha: e,
        offset: EVec2::ZERO,
        scale: 1.0,
        angle: 0.0,
        blur: 0.0,
        glow: 1.0,
    };
    match school {
        School::Pyromancy => {
            p.offset = EVec2::new(0.0, 6.0 * (1.0 - e));
            p.blur = 3.0 * (1.0 - e);
        }
        School::Arcane => {
            p.angle = -std::f32::consts::PI * (1.0 - e);
            p.scale = 0.3 + 0.7 * e;
            p.blur = 2.0 * (1.0 - e);
        }
        School::Umbral => {
            p.blur = 9.0 * (1.0 - e);
            p.scale = 1.6 - 0.6 * e;
        }
        School::Storm => {
            // On, nearly off, on: a crackle, bright at first.
            p.alpha = if k < 0.3 {
                k / 0.3
            } else if k < 0.45 {
                0.2
            } else {
                1.0
            };
            p.glow = 1.0 + 3.0 * (1.0 - k);
            p.blur = 2.0 * (1.0 - k);
        }
        School::Fae => {
            let (s, c) = (seed * 12.9898).sin_cos();
            p.offset = EVec2::new(c * 28.0, s * 18.0) * (1.0 - e);
            p.scale = 0.2 + 0.8 * e;
        }
        School::Druidic => {
            // Grows up from its baseline.
            p.scale = e.max(0.05);
            p.offset = EVec2::new(0.0, FONT_SIZE * 0.5 * (1.0 - e));
        }
        School::Astral => {
            p.alpha = if k < 0.4 {
                0.3 * k / 0.4
            } else {
                0.3 + 0.7 * (k - 0.4) / 0.6
            };
        }
        School::Warding => {
            // Etched: each letter snaps in as the cut passes it (the
            // stagger carries the cut along the line).
            p.alpha = if k > 0.0 { 1.0 } else { 0.0 };
            p.glow = 1.0 + 1.5 * (1.0 - k);
        }
    }
    p
}

pub struct Whispers {
    ctx: egui::Context,
    renderer: egui_wgpu::Renderer,
    line: Option<(String, School, Instant)>,
}

impl Whispers {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Whispers {
        let ctx = egui::Context::default();
        Whispers {
            ctx,
            renderer: egui_wgpu::Renderer::new(device, format, Default::default()),
            line: None,
        }
    }

    /// Lets the font be swapped in later (Tiny5, once it's bundled).
    pub fn set_fonts(&self, fonts: egui::FontDefinitions) {
        self.ctx.set_fonts(fonts);
    }

    pub fn show(&mut self, text: &str, school: School) {
        self.line = Some((text.to_string(), school, Instant::now()));
    }

    /// Draws the current whisper (if any) over `target`, which already holds the eye.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: (u32, u32),
        scale: f32,
        colors: [[f32; 3]; 4],
        fade: f32,
    ) {
        let Some((text, school, since)) = &self.line else {
            return;
        };
        let age = since.elapsed().as_secs_f32();
        let entrance_ends = school.entrance() + STAGGER_SECS * text.chars().count() as f32;
        let all_in = entrance_ends.max(HOLD_SECS);
        if age > all_in + FADE_SECS {
            self.line = None;
            return;
        }
        let leave = 1.0 - ((age - all_in) / FADE_SECS).clamp(0.0, 1.0);
        let (text, school) = (text.clone(), *school);

        let points = EVec2::new(size.0 as f32, size.1 as f32) / scale;
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, points)),
            ..Default::default()
        };
        raw.viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(scale);
        let c = |[r, g, b]: [f32; 3]| {
            Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
        };
        let (core, glow) = (c(colors[2]), c(colors[3]));
        let out = self.ctx.run_ui(raw, |ui| {
            let painter = ui.painter();
            let font = FontId::proportional(FONT_SIZE);
            // Lay the line out once to find each letter's place, centred
            // near the bottom, under the eye (the Electron app's CSS: bottom 8%).
            let galley = painter.layout_no_wrap(text.clone(), font.clone(), core);
            let origin = Pos2::new(
                (points.x - galley.size().x) / 2.0,
                points.y * 0.92 - galley.size().y,
            );
            // The glow breathes slowly (the studio's shimmer).
            let shimmer = 0.75 + 0.25 * (age * std::f32::consts::TAU / 3.2).sin();
            let glyphs = galley
                .rows
                .iter()
                .flat_map(|row| row.glyphs.iter().map(move |g| (row, g)));
            for (i, (row, g)) in glyphs.enumerate() {
                let t = age - i as f32 * STAGGER_SECS;
                if t <= 0.0 {
                    continue;
                }
                let p = pose(school, t, i as f32 + 1.0);
                let alpha = p.alpha * leave * fade;
                if alpha <= 0.0 {
                    continue;
                }
                let centre = origin
                    + row.pos.to_vec2()
                    + g.pos.to_vec2()
                    + EVec2::new(g.advance_width / 2.0, -g.font_ascent * 0.35)
                    + p.offset;
                let size = FONT_SIZE * p.scale;
                let ch = g.chr.to_string();
                let letter = |pos: Pos2, color: Color32| {
                    let galley =
                        painter.layout_no_wrap(ch.clone(), FontId::proportional(size), color);
                    let anchor = pos - galley.size() / 2.0;
                    let mut shape = egui::epaint::TextShape::new(anchor, galley, color);
                    shape.angle = p.angle;
                    painter.add(shape);
                };
                // Glow: a ring of faint copies; blur spreads them wider.
                let spread = 1.5 + p.blur;
                let ga = (0.22 * shimmer * p.glow).min(0.9) * alpha;
                for k in 0..8 {
                    let a = k as f32 / 8.0 * std::f32::consts::TAU;
                    letter(
                        centre + EVec2::new(a.cos(), a.sin()) * spread,
                        with_alpha(glow, ga),
                    );
                }
                letter(
                    centre,
                    with_alpha(core, alpha * (1.0 - (p.blur / 10.0)).clamp(0.3, 1.0)),
                );
            }
        });
        let prims = self.ctx.tessellate(out.shapes, out.pixels_per_point);
        let mut textures = out.textures_delta;
        for (id, deltas) in textures.set.drain() {
            for delta in &deltas {
                self.renderer.update_texture(device, queue, id, delta);
            }
        }
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [size.0, size.1],
            pixels_per_point: out.pixels_per_point,
        };
        let extra = self
            .renderer
            .update_buffers(device, queue, encoder, &prims, &screen);
        if !extra.is_empty() {
            queue.submit(extra);
        }
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("whispers"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target,
                        depth_slice: None,
                        resolve_target: None,
                        // Keep the eye underneath.
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            self.renderer.render(&mut pass, &prims, &screen);
        }
        for id in std::mem::take(&mut textures.free) {
            self.renderer.free_texture(&id);
        }
    }
}

fn with_alpha(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a.clamp(0.0, 1.0) * 255.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_school_ends_fully_in_place() {
        for school in [
            School::Pyromancy,
            School::Arcane,
            School::Umbral,
            School::Storm,
            School::Fae,
            School::Druidic,
            School::Astral,
            School::Warding,
        ] {
            let p = pose(school, school.entrance() + 0.01, 3.0);
            assert!((p.alpha - 1.0).abs() < 1e-4, "{school:?} alpha {}", p.alpha);
            assert!(p.offset.length() < 1e-3, "{school:?} offset {:?}", p.offset);
            assert!(
                (p.scale - 1.0).abs() < 1e-4 && p.angle.abs() < 1e-4 && p.blur.abs() < 1e-4,
                "{school:?}"
            );
        }
    }

    #[test]
    fn moods_map_to_the_studio_schools() {
        assert_eq!(School::of_mood("Watchful"), School::Pyromancy);
        assert_eq!(School::of_mood("Wrathful"), School::Storm);
        assert_eq!(School::of_mood("Blinded"), School::Warding);
    }

    #[test]
    fn every_letter_starts_hidden() {
        // Nothing should flash on screen before its entrance begins.
        for school in [
            School::Pyromancy,
            School::Arcane,
            School::Umbral,
            School::Storm,
            School::Fae,
            School::Druidic,
            School::Astral,
            School::Warding,
        ] {
            let p = pose(school, 0.0, 3.0);
            assert!(p.alpha.abs() < 1e-4, "{school:?} alpha {}", p.alpha);
        }
    }

    #[test]
    fn every_studio_mood_but_watchful_has_its_own_school() {
        // If the studio renames a mood, it would quietly fall back to
        // Pyromancy; this catches that.
        let moods = crate::mind::moods::Moods::load();
        let names: Vec<&str> = moods
            .theme()
            .moods
            .iter()
            .map(|m| m.name.as_str())
            .collect();
        assert!(!names.is_empty());
        for name in names.into_iter().filter(|&n| n != "Watchful") {
            assert_ne!(School::of_mood(name), School::Pyromancy, "{name}");
        }
    }
}
