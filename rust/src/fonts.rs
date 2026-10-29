//! Tiny5, the whisper font picked in the Blinki Studio: pixelated, but
//! magical. Built into the exe (SIL Open Font License; see assets/).

use std::sync::Arc;

static TINY5: &[u8] = include_bytes!("../assets/Tiny5-Regular.ttf");

/// egui's fonts with Tiny5 in front for ordinary text. egui's own fonts stay
/// behind it, so any character Tiny5 doesn't have still shows.
pub fn blinki() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert("Tiny5".into(), Arc::new(egui::FontData::from_static(TINY5)));
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        family.insert(0, "Tiny5".into());
    }
    fonts
}
