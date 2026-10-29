//! The Mind lens: mode, mood (and why), what it reads in your face and
//! remembers of you, the live numbers, and the whisper log.

use egui::{Align2, FontId, Pos2, Rect, Vec2 as EVec2};

use super::Snapshot;
use super::style::{Ink, bar};
use crate::mind::Mode;

pub(super) fn mind_lens(p: &egui::Painter, r: Rect, snap: &Snapshot, ink: &Ink) {
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
