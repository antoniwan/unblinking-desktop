//! The Hearing lens: loudness over time, the baseline, the bang line and
//! each flinch, with the Sound controls at its right.

use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke};

use super::style::{Ink, centered_note, with_alpha};
use super::{LEVEL_HISTORY, Snapshot};
use crate::audio::sound::Sound;
use crate::mind::BANG_LEVEL;

/// How wide the Sound controls are, at the right of the Hearing lens.
pub(super) const SOUND_WIDTH: f32 = 190.0;

pub(super) fn hearing_lens(p: &egui::Painter, r: Rect, snap: &Snapshot, ink: &Ink) {
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

/// Sound on/off, then Music, Voice and Effects, each with a switch and a
/// volume. Returns the new mixer if anything changed.
///
/// Unlike the lenses, which only paint, these are real egui widgets, so this
/// takes the `Ui` and not just a `Painter`.
pub(super) fn sound_controls(
    ui: &mut egui::Ui,
    r: Rect,
    mut sound: Sound,
    ink: &Ink,
) -> Option<Sound> {
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
