//! The Sight lens: the live camera picture with every face box and
//! landmark, the face it follows, the calibrated centre, and a gaze dial.

use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Stroke, StrokeKind, Vec2 as EVec2};

use super::Snapshot;
use super::style::{Ink, centered_note, with_alpha};
use crate::mind::Vec2;

pub(super) fn sight_lens(
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
