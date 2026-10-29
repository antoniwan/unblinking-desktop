//! "Where's my camera?": a small window that draws your monitors as they're
//! laid out, and lets you drag a camera to where your webcam really sits.
//! It snaps along the nearest edge, gently to its middle. Styled like
//! Blinki's Senses.

use std::error::Error;

use egui::{
    Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2 as EVec2,
};
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use crate::platform::desktop::Point;
use crate::senses::placement::{self, CameraSpot, Monitor};
use crate::ui::egui_window::EguiWindow;
use crate::ui::senses_panel::style::{Ink, arcane_circle, diamond, star, with_alpha};

/// What the window asks of the app after a frame.
pub enum Picked {
    Nothing,
    /// A new spot (or `None`: back to the default).
    Moved(Option<CameraSpot>),
    Close,
}

pub struct CameraPicker {
    win: EguiWindow,
    started: std::time::Instant,
}

impl CameraPicker {
    pub fn new(
        event_loop: &ActiveEventLoop,
        gpu: (&wgpu::Instance, &wgpu::Adapter, &wgpu::Device),
    ) -> Result<CameraPicker, Box<dyn Error>> {
        let win = EguiWindow::new(
            event_loop,
            gpu,
            "Where's my camera?",
            (820.0, 560.0),
            (560.0, 400.0),
        )?;
        Ok(CameraPicker {
            win,
            started: std::time::Instant::now(),
        })
    }

    pub fn id(&self) -> WindowId {
        self.win.id()
    }

    pub fn on_event(&mut self, device: &wgpu::Device, event: &winit::event::WindowEvent) {
        self.win.on_event(device, event);
    }

    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        monitors: &[Monitor],
        spot: Option<&CameraSpot>,
        colors: &[[f32; 3]; 4],
    ) -> Picked {
        let mut picked = Picked::Nothing;
        let t = self.started.elapsed().as_secs_f32();
        self.win.draw(device, queue, |ui| {
            picked = picker(ui, monitors, spot, colors, t)
        });
        picked
    }
}

fn picker(
    ui: &mut egui::Ui,
    monitors: &[Monitor],
    spot: Option<&CameraSpot>,
    colors: &[[f32; 3]; 4],
    t: f32,
) -> Picked {
    let ink = Ink::of(colors);
    let full = ui.max_rect();
    let p = ui.painter().clone();
    p.rect_filled(full, 0.0, ink.paper);
    arcane_circle(&p, full, &ink, t * 10.0);

    let area = full.shrink(20.0);
    star(
        &p,
        area.left_top() + EVec2::new(10.0, 15.0),
        10.0,
        ink.flame,
    );
    p.text(
        area.left_top() + EVec2::new(28.0, 0.0),
        Align2::LEFT_TOP,
        "Where's my camera?",
        FontId::proportional(26.0),
        ink.flame,
    );
    p.text(
        area.left_top() + EVec2::new(4.0, 34.0),
        Align2::LEFT_TOP,
        "Drag the camera to where your webcam sits. It slides along the nearest edge and settles in the middle.",
        FontId::proportional(13.0),
        ink.faint,
    );

    // The monitors, scaled to fit, keeping their real arrangement.
    let stage = Rect::from_min_max(
        area.left_top() + EVec2::new(0.0, 70.0),
        area.right_bottom() - EVec2::new(0.0, 64.0),
    );
    if monitors.is_empty() {
        p.text(
            stage.center(),
            Align2::CENTER_CENTER,
            "No monitors found",
            FontId::proportional(15.0),
            ink.faint,
        );
        return Picked::Nothing;
    }
    let (lo, hi) = monitors.iter().fold(
        (Pos2::new(f32::MAX, f32::MAX), Pos2::new(f32::MIN, f32::MIN)),
        |(lo, hi), m| {
            let r = &m.rect;
            (
                Pos2::new(lo.x.min(r.x as f32), lo.y.min(r.y as f32)),
                Pos2::new(
                    hi.x.max((r.x + r.width) as f32),
                    hi.y.max((r.y + r.height) as f32),
                ),
            )
        },
    );
    let span = hi - lo;
    let k = (stage.width() / span.x).min(stage.height() / span.y) * 0.9;
    let offset = stage.center() - (lo + span / 2.0).to_vec2() * k;
    let to_ui = |x: f64, y: f64| Pos2::new(x as f32 * k + offset.x, y as f32 * k + offset.y);
    let to_desk = |q: Pos2| Point {
        x: ((q.x - offset.x) / k) as f64,
        y: ((q.y - offset.y) / k) as f64,
    };

    for m in monitors {
        let r = Rect::from_min_max(
            to_ui(m.rect.x, m.rect.y),
            to_ui(m.rect.x + m.rect.width, m.rect.y + m.rect.height),
        );
        p.rect_filled(r, CornerRadius::same(8), ink.panel);
        p.rect_stroke(
            r,
            CornerRadius::same(8),
            Stroke::new(1.5, with_alpha(ink.glow, 0.6)),
            StrokeKind::Inside,
        );
        let main = if m.primary { " · main" } else { "" };
        p.text(
            r.center() - EVec2::new(0.0, 10.0),
            Align2::CENTER_CENTER,
            format!("{:.0} × {:.0}{main}", m.rect.width, m.rect.height),
            FontId::proportional(16.0),
            ink.text,
        );
        p.text(
            r.center() + EVec2::new(0.0, 12.0),
            Align2::CENTER_CENTER,
            format!(
                "{} · {:.0} cm wide",
                placement::short_name(&m.name),
                m.rect.width / m.px_per_m * 100.0
            ),
            FontId::proportional(12.0),
            ink.faint,
        );
        // Faint ticks at each edge's middle: the spots it snaps to.
        for c in [
            r.center_top(),
            r.center_bottom(),
            r.left_center(),
            r.right_center(),
        ] {
            diamond(&p, c, 3.0, with_alpha(ink.glow, 0.5));
        }
    }

    // Drag (or click) anywhere: the camera goes to the nearest edge spot.
    let response = ui.allocate_rect(stage, Sense::click_and_drag());
    let mut picked = Picked::Nothing;
    let mut shown = spot.cloned().or_else(|| CameraSpot::default_for(monitors));
    if let Some(at) = response.interact_pointer_pos()
        && let Some(s) = placement::nearest_spot(to_desk(at), monitors)
    {
        if shown.as_ref() != Some(&s) {
            picked = Picked::Moved(Some(s.clone()));
        }
        shown = Some(s);
    }
    if let Some(s) = &shown
        && let Some((point, _)) = placement::locate(Some(s), monitors)
    {
        camera_icon(&p, to_ui(point.x, point.y), &ink, response.dragged());
        p.text(
            area.left_bottom() - EVec2::new(-4.0, 44.0),
            Align2::LEFT_BOTTOM,
            format!("Camera: {}", s.describe()),
            FontId::proportional(15.0),
            ink.core,
        );
    }
    p.text(
        area.left_bottom() - EVec2::new(-4.0, 22.0),
        Align2::LEFT_BOTTOM,
        "Saved as you go. For a finer aim, sit as usual and click \"Look at me\" in the tray.",
        FontId::proportional(12.0),
        ink.faint,
    );

    // Buttons, bottom right.
    let done = Rect::from_min_size(
        area.right_bottom() - EVec2::new(96.0, 34.0),
        EVec2::new(96.0, 34.0),
    );
    let reset = Rect::from_min_size(
        done.left_top() - EVec2::new(206.0, 0.0),
        EVec2::new(196.0, 34.0),
    );
    if button(ui, &p, reset, "Top middle of main", &ink) {
        picked = Picked::Moved(None);
    }
    if button(ui, &p, done, "Done", &ink) {
        picked = Picked::Close;
    }
    picked
}

/// A little webcam: a body with a glowing lens. Brighter while held.
fn camera_icon(p: &egui::Painter, c: Pos2, ink: &Ink, held: bool) {
    let body = Rect::from_center_size(c, EVec2::new(30.0, 18.0));
    for (w, a) in [(10.0, 0.06), (6.0, 0.1), (3.0, 0.18)] {
        p.circle_stroke(
            c,
            14.0 + w,
            Stroke::new(w, with_alpha(ink.glow, if held { a * 2.0 } else { a })),
        );
    }
    p.rect_filled(
        body,
        CornerRadius::same(6),
        Color32::from_rgb(0x22, 0x1e, 0x28),
    );
    p.rect_stroke(
        body,
        CornerRadius::same(6),
        Stroke::new(1.5, ink.flame),
        StrokeKind::Outside,
    );
    p.circle_filled(c, 6.0, ink.deep);
    p.circle_filled(c, 3.5, ink.core);
}

fn button(ui: &mut egui::Ui, p: &egui::Painter, r: Rect, label: &str, ink: &Ink) -> bool {
    let response = ui.allocate_rect(r, Sense::click());
    let hot = response.hovered();
    p.rect_filled(
        r,
        CornerRadius::same(8),
        if hot {
            with_alpha(ink.glow, 0.25)
        } else {
            ink.panel
        },
    );
    p.rect_stroke(
        r,
        CornerRadius::same(8),
        Stroke::new(1.0, with_alpha(ink.glow, 0.7)),
        StrokeKind::Inside,
    );
    p.text(
        r.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(14.0),
        ink.text,
    );
    response.clicked()
}
