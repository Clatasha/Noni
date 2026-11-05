//! Paint the approved Noni app icon in the interface.

use crate::egui::{Color32, ColorImage, Id, Painter, Rect, TextureHandle, TextureOptions, pos2, vec2};

pub fn paint(painter: &Painter, rect: Rect) {
    let size = rect.width().min(rect.height());
    let pixels = size * painter.ctx().pixels_per_point();
    if !pixels.is_finite() || pixels < 4.0 {
        return;
    }

    let ctx = painter.ctx();
    let id = Id::new("noni-app-icon");
    let cached = ctx.data(|data| data.get_temp::<TextureHandle>(id));
    let texture = if let Some(texture) = cached {
        texture
    } else {
        let Ok(png) = bv_png::decode(include_bytes!("../../../../packaging/icons/noni.png"))
        else {
            return;
        };
        let image = ColorImage::from_rgba_unmultiplied(
            [png.width as usize, png.height as usize],
            &png.to_rgba8(),
        );
        let texture = ctx.load_texture("Noni app icon", image, TextureOptions::LINEAR);
        ctx.data_mut(|data| data.insert_temp(id, texture.clone()));
        texture
    };

    painter.image(
        texture.id(),
        Rect::from_center_size(rect.center(), vec2(size, size)),
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}
