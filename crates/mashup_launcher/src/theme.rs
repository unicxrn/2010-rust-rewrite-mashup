//! MW2-styled palette, fonts and egui visuals.
use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId, Stroke};

pub const BG: Color32 = Color32::from_rgb(0x0b, 0x0d, 0x0a);
pub const PANEL: Color32 = Color32::from_rgba_premultiplied(0x0f, 0x11, 0x0c, 0xd1);
pub const OLIVE: Color32 = Color32::from_rgb(0x9f, 0xbf, 0x3b);
pub const OLIVE_DIM: Color32 = Color32::from_rgb(0x5d, 0x6f, 0x24);
pub const TEXT: Color32 = Color32::from_rgb(0xe8, 0xea, 0xdf);
pub const MUTED: Color32 = Color32::from_rgb(0x8a, 0x8f, 0x7e);
pub const WARN: Color32 = Color32::from_rgb(0xe0, 0xa9, 0x2e);
pub const DANGER: Color32 = Color32::from_rgb(0xd9, 0x48, 0x2b);
pub const GRASS: Color32 = Color32::from_rgb(0x5f, 0xae, 0x3e);

const OXANIUM: &[u8] = include_bytes!("../../ui/assets/Oxanium-Regular.ttf");
const MONO: &[u8] = include_bytes!("../../console/assets/FreeMono.otf");

pub fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("display".into()))
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    // The "display" family needs to fall back to the default proportional
    // fonts after Oxanium, so glyphs Oxanium lacks (▶ U+25B6, box-drawing,
    // etc.) still render instead of showing tofu boxes.
    let default_proportional = fonts
        .families
        .get(&FontFamily::Proportional)
        .expect("default font families include Proportional")
        .clone();
    fonts
        .font_data
        .insert("oxanium".into(), FontData::from_static(OXANIUM).into());
    fonts
        .font_data
        .insert("mono".into(), FontData::from_static(MONO).into());
    let mut display_family = vec!["oxanium".to_owned()];
    display_family.extend(default_proportional);
    fonts
        .families
        .insert(FontFamily::Name("display".into()), display_family);
    fonts
        .families
        .get_mut(&FontFamily::Proportional)
        .expect("default font families include Proportional")
        .insert(0, "oxanium".into());
    fonts
        .families
        .get_mut(&FontFamily::Monospace)
        .expect("default font families include Monospace")
        .insert(0, "mono".into());
    ctx.set_fonts(fonts);

    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = PANEL;
    visuals.override_text_color = Some(TEXT);
    visuals.selection.bg_fill = OLIVE_DIM;
    visuals.selection.stroke = Stroke::new(1.0, OLIVE);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(0x1a, 0x1f, 0x14);
    visuals.widgets.hovered.bg_fill = OLIVE_DIM;
    visuals.widgets.active.bg_fill = OLIVE;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, OLIVE_DIM);
    ctx.set_visuals(visuals);
}

/// Uppercase text with MW2's wide tracking. Uses THIN SPACE (U+2009), which
/// epaint maps to the space glyph via `thin_space_width`; Oxanium's cmap
/// doesn't cover HAIR SPACE (U+200A), which would otherwise draw tofu boxes
/// between every letter.
pub fn tracked(text: &str) -> String {
    text.to_uppercase()
        .chars()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join("\u{2009}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracked_uses_thin_space_not_hair_space() {
        let text = tracked("Play");
        assert!(
            !text.contains('\u{200a}'),
            "must not contain HAIR SPACE: {text:?}"
        );
        let allowed: std::collections::HashSet<char> =
            "PLAY".chars().chain(std::iter::once('\u{2009}')).collect();
        assert!(
            text.chars().all(|c| allowed.contains(&c)),
            "unexpected character in {text:?}"
        );
    }
}
