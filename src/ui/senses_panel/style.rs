//! The look Blinki's Senses and "Where's my camera?" share: the inks, the
//! turning arcane circle behind everything, the lens frame, and the small
//! drawn marks (star, diamond, bar, a centred note).

use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Stroke, StrokeKind, Vec2 as EVec2};

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

/// A thin meter, filled `v` (0..1) of the way in `color`.
pub(super) fn bar(p: &egui::Painter, r: Rect, v: f32, color: Color32, ink: &Ink) {
    p.rect_filled(r, CornerRadius::same(3), with_alpha(ink.text, 0.06));
    let fill = Rect::from_min_size(r.min, EVec2::new(r.width() * v.clamp(0.0, 1.0), r.height()));
    p.rect_filled(fill, CornerRadius::same(3), color);
}

/// A faint line of text in the middle of a lens, for when it has nothing to show.
pub(crate) fn centered_note(p: &egui::Painter, r: Rect, text: &str, ink: &Ink) {
    p.text(
        r.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(15.0),
        ink.faint,
    );
}
