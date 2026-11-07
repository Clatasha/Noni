//! Headless review snapshots. Uses egui's actual tessellated meshes and font
//! atlas without requiring a graphics driver on a build machine.

use crate::egui::{self, Color32, ColorImage, TextureId};
use std::collections::HashMap;

pub(crate) fn capture(name: &str, size: egui::Vec2, mut draw: impl FnMut(&mut egui::Ui)) {
    capture_with_pointer(name, size, None, &mut draw);
}

pub(crate) fn capture_with_pointer(
    name: &str,
    size: egui::Vec2,
    pointer: Option<egui::Pos2>,
    mut draw: impl FnMut(&mut egui::Ui),
) {
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(true);
    crate::apply_theme(&ctx);
    let mut textures: HashMap<TextureId, ColorImage> = HashMap::new();
    for frame in 0..4 {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            time: Some(frame as f64 / 60.0),
            ..Default::default()
        };
        if let Some(pointer) = pointer {
            input.events.push(egui::Event::PointerMoved(pointer));
        }
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .inner_rect = input.screen_rect;
        let mut out = ctx.run_ui(input, |ui| draw(ui));
        for (id, deltas) in &out.textures_delta.set {
            for delta in deltas {
                let egui::ImageData::Color(image) = &delta.image;
                if let Some([x, y]) = delta.pos {
                    let target = textures.get_mut(id).expect("texture to update");
                    for row in 0..image.size[1] {
                        let start = (row + y) * target.size[0] + x;
                        target.pixels[start..start + image.size[0]].copy_from_slice(
                            &image.pixels[row * image.size[0]..(row + 1) * image.size[0]],
                        );
                    }
                } else {
                    textures.insert(*id, image.as_ref().clone());
                }
            }
        }
        if frame == 3 {
            let mut image = image::RgbaImage::from_pixel(
                size.x as u32,
                size.y as u32,
                image::Rgba([20, 22, 27, 255]),
            );
            for clipped in ctx.tessellate(std::mem::take(&mut out.shapes), out.pixels_per_point) {
                let egui::epaint::Primitive::Mesh(mesh) = clipped.primitive else {
                    continue;
                };
                let Some(texture) = textures.get(&mesh.texture_id) else {
                    continue;
                };
                for triangle in mesh.indices.as_chunks::<3>().0 {
                    let v = [
                        mesh.vertices[triangle[0] as usize],
                        mesh.vertices[triangle[1] as usize],
                        mesh.vertices[triangle[2] as usize],
                    ];
                    let clip = clipped
                        .clip_rect
                        .intersect(egui::Rect::from_min_size(egui::Pos2::ZERO, size));
                    raster(&mut image, texture, v, clip);
                }
            }
            let dir =
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/patch-review");
            std::fs::create_dir_all(&dir).unwrap();
            image.save(dir.join(format!("{name}.png"))).unwrap();
        }
        for id in &out.textures_delta.free {
            textures.remove(id);
        }
        out.drop_without_applying_deltas();
    }
}

fn raster(
    image: &mut image::RgbaImage,
    tex: &ColorImage,
    v: [egui::epaint::Vertex; 3],
    clip: egui::Rect,
) {
    let cross = |a: egui::Vec2, b: egui::Vec2| a.x * b.y - a.y * b.x;
    let area = cross(v[1].pos - v[0].pos, v[2].pos - v[0].pos);
    if area.abs() < 0.0001 {
        return;
    }
    let min_x = v
        .iter()
        .map(|p| p.pos.x)
        .fold(f32::INFINITY, f32::min)
        .max(clip.left())
        .floor()
        .max(0.0) as u32;
    let max_x = v
        .iter()
        .map(|p| p.pos.x)
        .fold(f32::NEG_INFINITY, f32::max)
        .min(clip.right())
        .ceil()
        .min(image.width() as f32) as u32;
    let min_y = v
        .iter()
        .map(|p| p.pos.y)
        .fold(f32::INFINITY, f32::min)
        .max(clip.top())
        .floor()
        .max(0.0) as u32;
    let max_y = v
        .iter()
        .map(|p| p.pos.y)
        .fold(f32::NEG_INFINITY, f32::max)
        .min(clip.bottom())
        .ceil()
        .min(image.height() as f32) as u32;
    for y in min_y..max_y {
        for x in min_x..max_x {
            let p = egui::pos2(x as f32 + 0.5, y as f32 + 0.5);
            let a = cross(v[1].pos - p, v[2].pos - p) / area;
            let b = cross(v[2].pos - p, v[0].pos - p) / area;
            let c = 1.0 - a - b;
            if a < 0.0 || b < 0.0 || c < 0.0 {
                continue;
            }
            let uv = v[0].uv.to_vec2() * a + v[1].uv.to_vec2() * b + v[2].uv.to_vec2() * c;
            let tx = uv.x * tex.size[0] as f32 - 0.5;
            let ty = uv.y * tex.size[1] as f32 - 0.5;
            let sample = |x: f32, y: f32| -> Color32 {
                let x = x.clamp(0.0, (tex.size[0] - 1) as f32) as usize;
                let y = y.clamp(0.0, (tex.size[1] - 1) as f32) as usize;
                tex.pixels[y * tex.size[0] + x]
            };
            let samples = [
                sample(tx.floor(), ty.floor()),
                sample(tx.ceil(), ty.floor()),
                sample(tx.floor(), ty.ceil()),
                sample(tx.ceil(), ty.ceil()),
            ];
            let fx = tx - tx.floor();
            let fy = ty - ty.floor();
            let weights = [
                (1.0 - fx) * (1.0 - fy),
                fx * (1.0 - fy),
                (1.0 - fx) * fy,
                fx * fy,
            ];
            let mut src = [0.0; 4];
            for ch in 0..4 {
                let texture: f32 = samples
                    .iter()
                    .zip(weights)
                    .map(|(s, w)| s[ch] as f32 * w)
                    .sum();
                let color = v[0].color[ch] as f32 * a
                    + v[1].color[ch] as f32 * b
                    + v[2].color[ch] as f32 * c;
                src[ch] = texture * color / 255.0;
            }
            let dest = image.get_pixel_mut(x, y);
            for ch in 0..4 {
                dest[ch] =
                    (src[ch] + dest[ch] as f32 * (1.0 - src[3] / 255.0)).clamp(0.0, 255.0) as u8;
            }
        }
    }
}
