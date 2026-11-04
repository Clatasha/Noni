//! Noni's fruit mark, drawn in its own colors on a transparent background.

use std::sync::OnceLock;

use crate::egui::{Color32, Mesh, Painter, Pos2, Rect, Shape, Vec2, pos2, vec2};

use super::path::{self, Sub};

/// The centered square of the design that contains the fruit.
const FROM: f32 = 75.0;
const SPAN: f32 = 450.0;
/// Under this many pixels across, nothing is drawn.
const LEAST_PIXELS: f32 = 4.0;
/// How far the corner of a soft edge may reach where the outline turns back
/// sharply, as a share of the bend (a bend of 110° would reach 2.4 times the
/// edge's width): the edge stays near the point instead of shooting out of it.
const LEAST_BEND: f32 = 0.35;

const STEM: Color32 = Color32::from_rgb(0x72, 0x9A, 0x55);
const RIND: Color32 = Color32::from_rgb(0x8C, 0xB5, 0x59);
const FLESH: Color32 = Color32::from_rgb(0xDD, 0xE9, 0x8E);
const DIMPLES: Color32 = Color32::from_rgb(0x4E, 0x71, 0x3B);

/// The vector shapes in paint order.
const SHAPES: [(&str, Color32); 15] = [
    (
        "M284 210C279 160 307 112 347 90C359 83 371 96 362 108C333 138 315 174 322 207Z",
        STEM,
    ),
    (
        "M302 191C336 169 365 177 386 199C419 198 444 224 443 253C467 273 475 305 462 330C476 357 466 389 445 409C446 439 425 463 399 470C383 500 354 513 327 506C305 528 271 525 251 504C222 506 196 490 185 465C154 456 142 431 146 405C123 385 119 354 132 328C120 301 130 271 152 253C152 223 179 200 207 201C231 177 271 173 302 191Z",
        RIND,
    ),
    (
        "M301 205C334 183 359 192 380 215C411 209 433 232 428 260C448 276 460 305 442 326C453 352 441 378 420 390C423 417 404 441 378 442C365 470 340 481 312 473C290 494 264 484 248 463C221 467 203 452 194 429C167 427 155 407 160 382C141 363 142 337 157 320C144 298 155 274 174 263C171 237 192 218 219 220C237 197 275 190 301 205Z",
        FLESH,
    ),
    ("M208 264C208 245 239 245 239 264C239 283 208 283 208 264Z", DIMPLES),
    ("M286 244C286 225 317 225 317 244C317 263 286 263 286 244Z", DIMPLES),
    ("M365 266C365 247 396 247 396 266C396 285 365 285 365 266Z", DIMPLES),
    ("M175 329C175 310 206 310 206 329C206 348 175 348 175 329Z", DIMPLES),
    ("M250 312C250 293 281 293 281 312C281 331 250 331 250 312Z", DIMPLES),
    ("M330 318C330 299 361 299 361 318C361 337 330 337 330 318Z", DIMPLES),
    ("M401 330C401 311 432 311 432 330C432 349 401 349 401 330Z", DIMPLES),
    ("M215 387C215 368 246 368 246 387C246 406 215 406 215 387Z", DIMPLES),
    ("M293 383C293 364 324 364 324 383C324 402 293 402 293 383Z", DIMPLES),
    ("M367 393C367 374 398 374 398 393C398 412 367 412 367 393Z", DIMPLES),
    ("M257 448C257 429 288 429 288 448C288 467 257 467 257 448Z", DIMPLES),
    ("M332 449C332 430 363 430 363 449C363 468 332 468 332 449Z", DIMPLES),
];

/// The shapes' paths, read the first time the logo is drawn.
fn shapes() -> &'static [(Vec<Sub>, Color32)] {
    static READ: OnceLock<Vec<(Vec<Sub>, Color32)>> = OnceLock::new();
    READ.get_or_init(|| {
        SHAPES
            .iter()
            .map(|&(data, color)| (path::parse(data), color))
            .collect()
    })
}

/// Draws the logo inside `rect` (the square that fits it, centered), in its
/// own colors and on no background.
pub fn paint(painter: &Painter, rect: Rect) {
    let s = rect.width().min(rect.height());
    let ppp = painter.ctx().pixels_per_point();
    if !(s * ppp).is_finite() || s * ppp < LEAST_PIXELS {
        return;
    }
    let unit = s / SPAN;
    // The square's corner on a whole pixel, as the icons' is.
    let snap = |v: f32| (v * ppp).round() / ppp;
    let corner = rect.center() - vec2(s, s) / 2.0;
    let corner = pos2(snap(corner.x), snap(corner.y));
    let to_px = |p: Pos2| corner + (p - pos2(FROM, FROM)) * unit;
    for (subs, color) in shapes() {
        for sub in subs {
            let (points, _) = super::tidy(sub.flatten(&to_px), true, ppp, false);
            smooth_fill(painter, &points, *color);
        }
    }
}

/// Fills a polygon (concave ones too) so that its outline is where the color
/// is half there: whole half a pixel inside it and gone half a pixel outside,
/// which is what the coverage of a rasterizer gives it.
fn smooth_fill(painter: &Painter, points: &[Pos2], color: Color32) {
    let n = points.len();
    if n < 3 {
        return;
    }
    let winding: f32 = (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum();
    // The normal of the side from point `i` to the next, pointing out.
    let out = |i: usize| -> Vec2 {
        let along = (points[(i + 1) % n] - points[i]).normalized();
        let normal = vec2(along.y, -along.x);
        if winding >= 0.0 { normal } else { -normal }
    };
    // Half a pixel: the soft edge is a pixel wide, centered on the outline.
    let half = 0.5 / painter.ctx().pixels_per_point();
    let bends: Vec<Vec2> = (0..n)
        .map(|i| {
            let (before, after) = (out((i + n - 1) % n), out(i));
            // The corner of the edge, as far from both sides as the edge is
            // wide.
            (before + after) / (1.0 + before.dot(after)).max(LEAST_BEND)
        })
        .collect();
    let mut mesh = Mesh::default();
    for (&p, &bend) in points.iter().zip(&bends) {
        mesh.colored_vertex(p - bend * half, color);
    }
    for (&p, &bend) in points.iter().zip(&bends) {
        mesh.colored_vertex(p + bend * half, Color32::TRANSPARENT);
    }
    // The inside is the polygon's own triangles, moved in with its points.
    mesh.indices = path::triangulate(points);
    for i in 0..n {
        let j = (i + 1) % n;
        let (a, b, c, d) = (i as u32, j as u32, (n + i) as u32, (n + j) as u32);
        mesh.add_triangle(a, b, d);
        mesh.add_triangle(a, d, c);
    }
    painter.add(Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_logo_has_closed_shapes_inside_its_box() {
        let same = |p: Pos2| p;
        assert_eq!(shapes().len(), SHAPES.len());
        for (subs, _) in shapes() {
            assert_eq!(subs.len(), 1);
            assert!(subs[0].closed);
            for p in subs[0].flatten(&same) {
                let inside = FROM..=FROM + SPAN;
                assert!(
                    inside.contains(&p.x) && inside.contains(&p.y),
                    "a point outside the logo's box: {p:?}"
                );
            }
        }
    }

    /// The logo draws at any size and scale: shapes with finite points in
    /// the logo's colors, and none at all where there is no room.
    #[test]
    fn the_logo_paints_at_any_size_and_scale() {
        use crate::egui::{Context, RawInput, epaint::Primitive};
        for ppp in [1.0, 1.5, 2.0] {
            let ctx = Context::default();
            ctx.set_pixels_per_point(ppp);
            for size in [3.0, 8.0, 14.0, 16.0, 20.0, 26.0, 33.0, 84.0, 200.0, 600.0] {
                let rect = Rect::from_min_size(pos2(10.3, 7.7), vec2(size, size));
                let mut out = ctx.run_ui(RawInput::default(), |ui| paint(ui.painter(), rect));
                out.textures_delta.clear();
                let painted = ctx.tessellate(out.shapes, ppp);
                assert_eq!(painted.is_empty(), size * ppp < LEAST_PIXELS, "{size} px");
                let mut colors = Vec::new();
                for primitive in painted {
                    let Primitive::Mesh(mesh) = primitive.primitive else {
                        continue;
                    };
                    assert!(mesh.vertices.iter().all(|v| v.pos.is_finite()));
                    assert!(
                        mesh.indices
                            .iter()
                            .all(|&i| (i as usize) < mesh.vertices.len())
                    );
                    colors.extend(mesh.vertices.iter().map(|v| v.color));
                }
                if size >= 84.0 {
                    for color in [STEM, RIND, FLESH, DIMPLES] {
                        assert!(colors.contains(&color), "{size} px has no {color:?}");
                    }
                }
            }
        }
    }
}
