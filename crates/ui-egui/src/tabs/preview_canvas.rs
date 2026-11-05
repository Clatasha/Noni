//! Direct clip framing and the canvas-size menu for the main preview.

use bv_core::Tick;
use bv_timeline::pancrop::{self, PanCropKey, PlaceIn, Placement};
use bv_timeline::{ClipId, OutputRotation, PixelAspect, Project, TrackKind, Transform};

use crate::app::BananaApp;
use crate::egui::{self, Pos2, Rect, Stroke, Vec2, pos2, vec2};
use crate::i18n::tr;
use crate::icons::Icon;
use crate::theme;

const RATIOS: [(&str, u32, u32); 6] = [
    ("16:9", 1920, 1080),
    ("9:16", 1080, 1920),
    ("1:1", 1080, 1080),
    ("4:5", 1080, 1350),
    ("4:3", 1440, 1080),
    ("21:9", 2520, 1080),
];

pub(super) fn ratio_menu(ui: &mut egui::Ui, app: &mut BananaApp) {
    let settings = app.project().settings;
    let button = crate::widgets::icon_button_ex(
        ui,
        Icon::ShapeWide,
        tr("Canvas Ratio"),
        false,
        !settings.output_360,
    );
    let mut chosen = None;
    egui::Popup::menu(&button).show(|ui| {
        ui.strong(tr("Canvas Ratio"));
        let (w, h) = settings.output_size();
        for (ratio, width, height) in RATIOS {
            let selected = u64::from(w) * u64::from(height) == u64::from(h) * u64::from(width)
                && settings.pixel_aspect.is_square();
            let label = format!("{ratio}   ·   {width} × {height}");
            if ui.selectable_label(selected, label).clicked() {
                chosen = Some((width, height));
                ui.close();
            }
        }
    });
    if let Some((width, height)) = chosen {
        app.modify(|project| resize_canvas(project, width, height));
    }
}

/// Frame-sized Pan/Crop boxes hold ordinary position and scale edits. Refit
/// them to the new canvas while keeping those values. Source-sized and custom
/// crop boxes already fit the new frame through the normal placement path.
fn resize_canvas(project: &mut Project, width: u32, height: u32) {
    let before = project.settings;
    let mut after = before;
    let (width, height) = if before.rotation.is_quarter_turn() {
        (height, width)
    } else {
        (width, height)
    };
    after.width = width;
    after.height = height;
    after.pixel_aspect = PixelAspect::SQUARE;
    if before == after {
        return;
    }
    let old_ratio = f64::from(before.width) * f64::from(before.pixel_aspect.ratio())
        / f64::from(before.height);
    let new_ratio = f64::from(after.width) / f64::from(after.height);
    if (old_ratio / new_ratio - 1.0).abs() > 1e-6 {
        let media: std::collections::HashMap<_, _> =
            project.media.iter().map(|info| (info.id, info)).collect();
        for track in &mut project.tracks {
            if track.kind != TrackKind::Video {
                continue;
            }
            for clip in &mut track.clips {
                let pan = &mut clip.pan_crop;
                if !pan.keep_aspect || !pan.fill || pan.keys.is_empty() {
                    continue;
                }
                let Some(&info) = media.get(&clip.media) else {
                    continue;
                };
                let old = pancrop::place_in(&before, info, true, true);
                let new = pancrop::place_in(&after, info, true, true);
                let stored_par = if pan.size[0] > 0 && pan.size[1] > 0 {
                    old.media_aspect * old.media[0] / old.media[1]
                        * f64::from(pan.size[1]) / f64::from(pan.size[0])
                } else {
                    old.media_aspect
                };
                let matches_canvas = |key: &PanCropKey, par: f64| {
                    let ratio = key.width.abs() * par / key.height.abs();
                    (ratio / old_ratio - 1.0).abs() <= 1e-4
                };
                if !pan.keys.iter().any(|key| matches_canvas(key, stored_par)) {
                    continue;
                }
                pan.ensure_size(old.media);
                for key in &mut pan.keys {
                    if !matches_canvas(key, old.media_aspect) {
                        continue;
                    }
                    let tf = pancrop::transform_from_key(key, &old);
                    let framed = framed_key(&tf, &new, key);
                    key.width = framed.width;
                    key.height = framed.height;
                    key.center = framed.center;
                    key.pivot = framed.pivot;
                }
            }
        }
    }
    project.settings = after;
}

fn framed_key(tf: &Transform, place: &PlaceIn, original: &PanCropKey) -> PanCropKey {
    let mut key = pancrop::key_from_transform(tf, place);
    key.at = original.at;
    key.curve = original.curve;
    key.smoothness = original.smoothness;
    key.pivot = [
        key.center[0] + original.pivot[0] - original.center[0],
        key.center[1] + original.pivot[1] - original.center[1],
    ];
    key
}

const HANDLE_RADIUS: f32 = 4.0;
const HANDLE_HIT: f32 = 10.0;
const SNAP_DISTANCE: f32 = 8.0;
const ROTATE_RADIUS: f32 = 8.0;
const ROTATE_OFFSET: f32 = 23.0;

#[derive(Clone, Copy)]
enum Tool {
    Move,
    Resize(usize),
    Rotate,
}

#[derive(Clone, Copy)]
struct Drag {
    id: ClipId,
    time: Tick,
    edit_at: Tick,
    start: Pos2,
    previous: Pos2,
    rotation_delta: f32,
    frame: Rect,
    corners: [Pos2; 4],
    center: Pos2,
    original: PanCropKey,
    transform: Transform,
    tool: Tool,
    undo: egui::Id,
}

pub(super) fn clip_handles(
    ui: &egui::Ui,
    app: &mut BananaApp,
    nav: &egui::Response,
    frame: Rect,
    area: Rect,
) {
    let state = ui.id().with("canvas-framing-drag");
    let mut active = ui.data(|data| data.get_temp::<Drag>(state));
    let time = app.display_time();
    let target = app.primary_clip().and_then(|id| {
        let p = app.project();
        let track = p.clip_track(id)?;
        let clip = p.clip(id)?;
        let place = p.clip_place_in(id)?;
        if app.is_playing()
            || track.kind != TrackKind::Video
            || track.locked
            || track.muted
            || clip.locked
            || clip.muted
            || time < clip.start
            || time >= clip.end()
            || !place.fill
            || frame.width() < 1.0
            || frame.height() < 1.0
        {
            return None;
        }
        let key = clip.box_at(time, place.media, place.media_aspect);
        // A static clip stays static even when the playhead is mid-clip.
        let edit_at = match clip.pan_crop.keys.as_slice() {
            [] => clip.start,
            [key] => clip.start + key.at,
            _ => time,
        };
        let (slide, _) = bv_timeline::scene::clip_animation(clip, time);
        Some((id, place, key, edit_at, slide))
    });
    let Some((id, place, key, edit_at, slide)) = target else {
        ui.data_mut(|data| data.remove::<Drag>(state));
        return;
    };
    let rotation = app.project().settings.rotation;
    if active.is_some_and(|drag| drag.id != id || drag.time != time || drag.frame != frame) {
        active = None;
    }
    let initial = pancrop::place(&key, &place);
    let Some(mut corners) = outline(&initial, &place, rotation, frame, slide)
        .or_else(|| active.map(|drag| drag.corners))
    else {
        ui.data_mut(|data| data.remove::<Drag>(state));
        return;
    };
    let primary = egui::PointerButton::Primary;
    if nav.drag_started_by(primary)
        && let Some(start) = ui.input(|input| input.pointer.press_origin())
        && let Some(tool) = tool_at(start, &corners, area)
    {
        active = Some(Drag {
            id,
            time,
            edit_at,
            start,
            previous: start,
            rotation_delta: 0.0,
            frame,
            corners,
            center: source_point(
                &initial,
                &place,
                rotation,
                frame,
                place.media.map(|v| v * 0.5),
                slide,
            ),
            original: key,
            transform: pancrop::transform_from_key(&key, &place),
            tool,
            undo: state.with(ui.ctx().cumulative_pass_nr_for(egui::ViewportId::ROOT)),
        });
    }
    let tool = active
        .map(|drag| drag.tool)
        .or_else(|| nav.hover_pos().and_then(|pos| tool_at(pos, &corners, area)));
    if let Some(tool) = tool {
        let cursor = match tool {
            Tool::Move => egui::CursorIcon::Move,
            Tool::Rotate if active.is_some() => egui::CursorIcon::Grabbing,
            Tool::Rotate => {
                nav.clone().on_hover_text(tr("Rotate clip"));
                egui::CursorIcon::Grab
            }
            Tool::Resize(i) => {
                let delta = corners[i] - corners[(i + 2) % 4];
                if delta.x * delta.y >= 0.0 {
                    egui::CursorIcon::ResizeNwSe
                } else {
                    egui::CursorIcon::ResizeNeSw
                }
            }
        };
        ui.ctx().set_cursor_icon(cursor);
    }

    let mut guides = [false; 2];
    if let Some(mut drag) = active
        && (nav.dragged_by(primary) || nav.drag_stopped_by(primary))
        && let Some(pointer) = nav
            .interact_pointer_pos()
            .or_else(|| ui.input(|i| i.pointer.latest_pos()))
    {
        let mut delta = pointer - drag.start;
        let mut tf = drag.transform;
        match drag.tool {
            Tool::Move => {
                if app.snapping && !ui.input(|input| input.modifiers.shift) {
                    let center = drag.corners[0].lerp(drag.corners[2], 0.5);
                    let snap = frame.center() - (center + delta);
                    guides = [snap.x.abs() <= SNAP_DISTANCE, snap.y.abs() <= SNAP_DISTANCE];
                    if guides[0] {
                        delta.x += snap.x;
                    }
                    if guides[1] {
                        delta.y += snap.y;
                    }
                }
            }
            Tool::Resize(i) => {
                let anchor = drag.corners[(i + 2) % 4];
                let diagonal = drag.corners[i] - anchor;
                let relative = 1.0 + delta.dot(diagonal) / diagonal.length_sq().max(1.0);
                let scale = drag.transform.scale;
                tf.scale = (scale * relative).clamp(0.05_f32.min(scale), 4.0_f32.max(scale));
                let factor = tf.scale / drag.transform.scale;
                delta = (anchor - drag.center) * (1.0 - factor);
            }
            Tool::Rotate => {
                let before = drag.previous - drag.center;
                let after = pointer - drag.center;
                // A pointer through the pivot has no meaningful angle.
                if after.length_sq() >= 9.0 {
                    drag.rotation_delta += turn_between(before, after);
                    drag.previous = pointer;
                }
                tf.rotation_deg += drag.rotation_delta;
                delta = Vec2::ZERO;
                active = Some(drag);
            }
        }
        let normalized = from_output_delta(delta / frame.size(), rotation) / place.adjust as f32;
        tf.x += normalized.x;
        tf.y -= normalized.y;
        let next = if tf == drag.transform {
            drag.original
        } else {
            framed_key(&tf, &place, &drag.original)
        };
        if key != next {
            let preferences = app.settings.prefs.new_motion_keys();
            app.modify_merged(drag.undo, |project| {
                let _ = project.set_clip_box_at(id, drag.edit_at, next, preferences);
            });
        }
        if let Some(next) = outline(&pancrop::place(&next, &place), &place, rotation, frame, slide) {
            corners = next;
        }
    }
    if ui.input(|input| input.pointer.button_down(primary))
        && let Some(drag) = active
    {
        ui.data_mut(|data| data.insert_temp(state, drag));
    } else {
        ui.data_mut(|data| data.remove::<Drag>(state));
    }

    let painter = ui.painter().with_clip_rect(area);
    let stroke = Stroke::new(1.5, theme::banana());
    if guides[0] {
        painter.vline(frame.center().x, frame.y_range(), stroke);
    }
    if guides[1] {
        painter.hline(frame.x_range(), frame.center().y, stroke);
    }
    painter.add(egui::Shape::closed_line(corners.to_vec(), stroke));
    for corner in corners {
        painter.circle_filled(corner, HANDLE_RADIUS, theme::panel_dark());
        painter.circle_stroke(corner, HANDLE_RADIUS, stroke);
    }
    if let Some((edge, handle)) = rotate_handle(&corners, area) {
        painter.line_segment([edge, handle], stroke);
        painter.circle_filled(handle, ROTATE_RADIUS, theme::panel_dark());
        painter.circle_stroke(handle, ROTATE_RADIUS, stroke);
        crate::icons::paint(
            &painter,
            Rect::from_center_size(handle, Vec2::splat(13.0)),
            Icon::Refresh,
            theme::banana(),
        );
    }
}

fn turn_between(before: Vec2, after: Vec2) -> f32 {
    let cross = before.x * after.y - before.y * after.x;
    cross.atan2(before.dot(after)).to_degrees()
}

fn rotate_handle(corners: &[Pos2; 4], area: Rect) -> Option<(Pos2, Pos2)> {
    let edge = corners[0].lerp(corners[1], 0.5);
    let center = corners[0].lerp(corners[2], 0.5);
    let offset = (edge - center).normalized() * ROTATE_OFFSET;
    let safe = area.shrink(ROTATE_RADIUS + 2.0);
    // When the frame touches the preview edge, put the handle just inside it.
    [edge + offset, edge - offset]
        .into_iter()
        .find(|&handle| safe.contains(handle))
        .map(|handle| (edge, handle))
}

fn tool_at(pos: Pos2, corners: &[Pos2; 4], area: Rect) -> Option<Tool> {
    if rotate_handle(corners, area)
        .is_some_and(|(_, handle)| handle.distance_sq(pos) <= HANDLE_HIT * HANDLE_HIT)
    {
        return Some(Tool::Rotate);
    }
    if let Some((i, _)) = corners
        .iter()
        .enumerate()
        .filter(|(_, corner)| corner.distance_sq(pos) <= HANDLE_HIT * HANDLE_HIT)
        .min_by(|(_, a), (_, b)| a.distance_sq(pos).total_cmp(&b.distance_sq(pos)))
    {
        return Some(Tool::Resize(i));
    }
    let sides: [f32; 4] = std::array::from_fn(|i| {
        let start = corners[i];
        let edge = corners[(i + 1) % 4] - start;
        let delta = pos - start;
        edge.x * delta.y - edge.y * delta.x
    });
    (sides.iter().all(|side| *side >= 0.0) || sides.iter().all(|side| *side <= 0.0))
        .then_some(Tool::Move)
}

fn outline(
    placement: &Placement,
    place: &PlaceIn,
    rotation: OutputRotation,
    frame: Rect,
    slide: f64,
) -> Option<[Pos2; 4]> {
    if placement.empty {
        return None;
    }
    // Keep handles on the media, including the portion outside the canvas.
    let [l, t, r, b] = placement.limits;
    if r <= l || b <= t {
        return None;
    }
    let corners = [[l, t], [r, t], [r, b], [l, b]]
        .map(|point| source_point(placement, place, rotation, frame, point, slide));
    (corners.iter().all(|p| p.x.is_finite() && p.y.is_finite())
        && corners[0].distance_sq(corners[2]) >= 1.0)
        .then_some(corners)
}

fn source_point(
    placement: &Placement,
    place: &PlaceIn,
    rotation: OutputRotation,
    frame: Rect,
    point: [f64; 2],
    slide: f64,
) -> Pos2 {
    let [x, y] = placement.on_frame_point(point);
    let (u, v) = ((x / place.frame[0]) as f32, (y / place.frame[1] + slide) as f32);
    let (u, v) = match rotation {
        OutputRotation::None => (u, v),
        OutputRotation::Clockwise90 => (1.0 - v, u),
        OutputRotation::Inverted180 => (1.0 - u, 1.0 - v),
        OutputRotation::Counterclockwise90 => (v, 1.0 - u),
    };
    pos2(
        frame.left() + u * frame.width(),
        frame.top() + v * frame.height(),
    )
}

fn from_output_delta(delta: Vec2, rotation: OutputRotation) -> Vec2 {
    match rotation {
        OutputRotation::None => delta,
        OutputRotation::Clockwise90 => vec2(delta.y, -delta.x),
        OutputRotation::Inverted180 => -delta,
        OutputRotation::Counterclockwise90 => vec2(-delta.y, delta.x),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotate_handle_stays_reachable_when_the_clip_fills_the_preview() {
        let area = Rect::from_min_size(Pos2::ZERO, vec2(400.0, 225.0));
        let corners = [
            area.left_top(),
            area.right_top(),
            area.right_bottom(),
            area.left_bottom(),
        ];
        let (_, handle) = rotate_handle(&corners, area).expect("rotate handle");
        assert!(area.shrink(ROTATE_RADIUS).contains(handle));
        assert!(matches!(
            tool_at(handle, &corners, area),
            Some(Tool::Rotate)
        ));
        assert!(matches!(
            tool_at(corners[0], &corners, area),
            Some(Tool::Resize(0))
        ));
    }

    #[test]
    fn dragging_the_rotate_handle_rotates_the_clip_and_undoes_in_one_step() {
        let ctx = egui::Context::default();
        let mut app = BananaApp::new(ctx.clone());
        app.load_demo();
        let clip = app.project().tracks[0].clips[0].clone();
        let id = clip.id;
        let at = clip.start;
        app.select_clip(id, false);
        app.seek(at);
        let place = app.project().clip_place_in(id).expect("clip placement");
        let original = clip.box_at(at, place.media, place.media_aspect);
        let area = Rect::from_min_size(pos2(12.0, 12.0), vec2(480.0, 300.0));
        let frame = Rect::from_center_size(area.center(), vec2(400.0, 225.0));
        let initial = pancrop::place(&original, &place);
        let output = app.project().settings.rotation;
        let (slide, _) = bv_timeline::scene::clip_animation(&clip, at);
        let corners = outline(&initial, &place, output, frame, slide).expect("clip outline");
        let (_, start) = rotate_handle(&corners, area).expect("rotate handle");
        let center = source_point(
            &initial,
            &place,
            output,
            frame,
            place.media.map(|v| v * 0.5),
            slide,
        );
        let offset = start - center;
        let end = center + vec2(-offset.y, offset.x);

        let mut frame_input = |events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(520.0, 340.0))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let nav = ui.interact(area, ui.id().with("preview-test"), egui::Sense::drag());
                    clip_handles(ui, &mut app, &nav, frame, area);
                },
            );
            output.textures_delta.clear();
        };
        let primary = |pos, pressed| egui::Event::PointerButton {
            pos,
            pressed,
            button: egui::PointerButton::Primary,
            modifiers: egui::Modifiers::NONE,
        };
        frame_input(vec![]);
        frame_input(vec![egui::Event::PointerMoved(start), primary(start, true)]);
        frame_input(vec![egui::Event::PointerMoved(start.lerp(end, 0.5))]);
        frame_input(vec![egui::Event::PointerMoved(end)]);
        frame_input(vec![primary(end, false)]);

        let rotated =
            app.project()
                .clip(id)
                .expect("clip")
                .box_at(at, place.media, place.media_aspect);
        let before = pancrop::transform_from_key(&original, &place);
        let after = pancrop::transform_from_key(&rotated, &place);
        assert!((after.rotation_deg - before.rotation_deg - 90.0).abs() < 0.01);
        assert!((after.x - before.x).abs() < 0.001 && (after.y - before.y).abs() < 0.001);
        assert!((after.scale - before.scale).abs() < 0.001);
        app.undo();
        assert_eq!(app.project().clip(id), Some(&clip));
    }
}
