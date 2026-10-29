//! The World lens: the cursor, the eye's window, the screen, idle time, the
//! clock and where the webcam is set to be.

use egui::{Align2, FontId, Pos2, Rect};

use super::Snapshot;
use super::style::Ink;
use crate::platform::desktop::Point;

pub(super) fn world_lens(p: &egui::Painter, r: Rect, snap: &Snapshot, ink: &Ink) {
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
