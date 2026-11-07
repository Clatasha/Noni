//! Bundled medium-weight digits for the timeline's primary time display.

use crate::egui::{self, FontData, FontDefinitions, FontFamily, FontId};

const FAMILY: &str = "noni-timecode-oswald-medium";

pub(crate) fn install(ctx: &egui::Context) {
    let id = egui::Id::new(FAMILY);
    if ctx.data_mut(|d| d.get_temp::<bool>(id).unwrap_or(false)) {
        return;
    }
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        FAMILY.into(),
        FontData::from_static(include_bytes!("../assets/fonts/Oswald-Medium.ttf")).into(),
    );
    let mut fallbacks = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    fallbacks.insert(0, FAMILY.into());
    fonts
        .families
        .insert(FontFamily::Name(FAMILY.into()), fallbacks);
    ctx.set_fonts(fonts);
    ctx.data_mut(|d| d.insert_temp(id, true));
}

pub(crate) fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FAMILY.into()))
}

/// Numeric timecodes use fixed-width digit cells so the readout stays still
/// as narrow digits such as 1 replace wider ones such as 8. Glyphs retain
/// their original outlines and raster size; no text image is stretched.
pub(crate) struct Layout {
    parts: Vec<(f32, std::sync::Arc<egui::Galley>)>,
    pub size: egui::Vec2,
}

pub(crate) fn layout(
    painter: &egui::Painter,
    text: &str,
    size: f32,
    color: egui::Color32,
) -> Layout {
    let glyph = |text: String| painter.layout_no_wrap(text, font(size), color);
    let numeric = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || ":.; ,+-".contains(c));
    if !numeric {
        let galley = glyph(text.into());
        return Layout {
            size: galley.size(),
            parts: vec![(0.0, galley)],
        };
    }
    let cell = ('0'..='9')
        .map(|c| glyph(c.to_string()).size().x)
        .fold(0.0, f32::max);
    let mut parts = Vec::new();
    let mut width = 0.0;
    let mut height: f32 = 0.0;
    for c in text.chars() {
        let galley = glyph(c.to_string());
        let advance = if c.is_ascii_digit() {
            cell
        } else {
            galley.size().x
        };
        height = height.max(galley.size().y);
        parts.push((width + (advance - galley.size().x) * 0.5, galley));
        width += advance;
    }
    Layout {
        parts,
        size: egui::vec2(width, height),
    }
}

impl Layout {
    pub fn paint(self, painter: &egui::Painter, origin: egui::Pos2, color: egui::Color32) {
        let ppp = painter.ctx().pixels_per_point();
        for (offset, galley) in self.parts {
            let at = egui::pos2(
                ((origin.x + offset) * ppp).round() / ppp,
                (origin.y * ppp).round() / ppp,
            );
            painter.galley(at, galley, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_timecode_digits_are_stable_and_fit_without_scaling() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let ctx = egui::Context::default();
            install(&ctx);
            let mut input = egui::RawInput::default();
            input
                .viewports
                .get_mut(&egui::ViewportId::ROOT)
                .unwrap()
                .native_pixels_per_point = Some(scale);
            ctx.run_ui(input, |ui| {
                let painter = ui.painter();
                let readout = |text| layout(painter, text, 24.0, egui::Color32::WHITE);
                assert_eq!(
                    readout("11:11:11.111").size.x,
                    readout("88:88:88.888").size.x
                );
                let time = readout("00:14:14.209");
                assert!(time.size.x < 180.0);
                assert!(time.size.y <= 36.0);
                assert_eq!(
                    time.parts[0].1.job.sections[0].format.font_id.family,
                    font(24.0).family
                );
                time.paint(painter, egui::pos2(10.25, 10.25), egui::Color32::WHITE);
            })
            .drop_without_applying_deltas();
        }
    }
}
