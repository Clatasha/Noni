//! The Classic video and image inspector.

use super::{orientation_strip, value_slider};
use crate::egui::{self, Color32, RichText, Ui, vec2};
use crate::{app::BananaApp, i18n::tr, theme};
use bv_core::{Tick, timecode};
use bv_timeline::envelope::{Envelope, EnvelopeKind};
use bv_timeline::pancrop::{self, PanCropKey, PlaceIn};
use bv_timeline::{Clip, CompositeMode, Transform};

const OPACITY: EnvelopeKind = EnvelopeKind::EventOpacity;

pub(super) fn ui(
    ui: &mut Ui,
    app: &mut BananaApp,
    clip: &mut Clip,
    at: Tick,
    place: Option<PlaceIn>,
    settings: impl FnOnce(&mut Ui, &mut Clip),
) {
    let id = clip.id;
    let rate = app.project().settings.frame_rate;
    let local = (rate.snap_nearest(at) - clip.start).clamp(Tick::ZERO, clip.duration);
    let preferences = app.settings.prefs.new_motion_keys();
    let footer_id = ui.id().with(("keyframe-panel", id.0));
    let mut show_keys = ui.data(|d| d.get_temp(footer_id)).unwrap_or(false);
    let scroll_height = (ui.available_height() - 38.0).max(48.0);
    egui::ScrollArea::vertical()
        .id_salt(("classic-visual-scroll", id.0))
        .max_height(scroll_height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add_enabled_ui(!clip.locked, |ui| {
                if let Some(place) = place {
                    transform_section(ui, clip, at, local, place, preferences);
                }
                compositing_section(
                    ui,
                    clip,
                    local,
                    app.settings.prefs.new_envelope_curve(OPACITY),
                );
                extras(ui, clip);
            });
            ui.add_space(12.0);
            if ui.button(tr("Open Pan/Crop…")).clicked() {
                app.clip_windows.pan_crop = Some(id);
            }
            ui.add_space(8.0);
            egui::CollapsingHeader::new(tr("Clip Settings")).show(ui, |ui| settings(ui, clip));
            if show_keys {
                body(ui, |ui| keyframe_panel(ui, app, clip));
                let scrolled = footer_id.with("scrolled");
                if !ui.data(|d| d.get_temp(scrolled)).unwrap_or(false) {
                    ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
                    ui.data_mut(|d| d.insert_temp(scrolled, true));
                }
            } else {
                ui.data_mut(|d| d.remove::<bool>(footer_id.with("scrolled")));
            }
        });
    ui.add_space(5.0);
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!clip.locked, egui::Button::new(tr("Reset")))
            .clicked()
        {
            reset_transform(clip);
            clip.visual = Default::default();
            clip.level = 1.0;
            clip.envelopes.retain(|env| env.kind != OPACITY);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(egui::Button::new(tr("Keyframe Panel")).selected(show_keys))
                .clicked()
            {
                show_keys = !show_keys;
            }
        });
    });
    ui.data_mut(|d| d.insert_temp(footer_id, show_keys));
}

fn reset_transform(clip: &mut Clip) {
    clip.pan_crop = Default::default();
    clip.visual.transform_keyed = false;
    clip.visual.transform_bypass = false;
}

fn header(
    ui: &mut Ui,
    title: &str,
    enabled: &mut bool,
    default_open: bool,
    actions: impl FnOnce(&mut Ui),
) -> bool {
    let id = ui.id().with(("section-open", title));
    let mut open = ui.data(|d| d.get_temp(id)).unwrap_or(default_open);
    egui::Frame::new()
        .fill(theme::hover())
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, response) =
                    ui.allocate_exact_size(vec2(24.0, 14.0), egui::Sense::click());
                if response.clicked() {
                    *enabled = !*enabled;
                }
                let color = if *enabled {
                    theme::banana()
                } else {
                    theme::text_dim()
                };
                ui.painter().rect(
                    rect.shrink(0.5),
                    7.0,
                    theme::panel_dark(),
                    egui::Stroke::new(1.0, color),
                    egui::StrokeKind::Inside,
                );
                let x = if *enabled {
                    rect.right() - 7.0
                } else {
                    rect.left() + 7.0
                };
                ui.painter()
                    .circle_filled(egui::pos2(x, rect.center().y), 4.1, color);
                response.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::Checkbox,
                        ui.is_enabled(),
                        *enabled,
                        title,
                    )
                });
                response.on_hover_text(tr("Enable or bypass"));
                let text = ui.painter().layout_no_wrap(
                    title.to_owned(),
                    egui::TextStyle::Button.resolve(ui.style()),
                    theme::text_strong(),
                );
                let (rect, collapse) =
                    ui.allocate_exact_size(vec2(text.size().x + 14.0, 20.0), egui::Sense::click());
                if collapse.clicked() {
                    open = !open;
                }
                let color = if collapse.hovered() {
                    theme::text_strong()
                } else {
                    theme::text_dim()
                };
                ui.painter().galley(
                    rect.left_center() - vec2(0.0, text.size().y / 2.0),
                    text,
                    color,
                );
                let c = rect.right_center() - vec2(5.0, 0.0);
                let arrow = if open {
                    [vec2(-3.0, -1.5), vec2(3.0, -1.5), vec2(0.0, 2.0)]
                } else {
                    [vec2(-1.5, -3.0), vec2(-1.5, 3.0), vec2(2.0, 0.0)]
                };
                ui.painter().add(egui::Shape::convex_polygon(
                    arrow.map(|point| c + point).to_vec(),
                    color,
                    egui::Stroke::NONE,
                ));
                collapse.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), title)
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), actions);
            });
        });
    ui.data_mut(|d| d.insert_temp(id, open));
    open
}

fn body(ui: &mut Ui, content: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(theme::panel())
        .inner_margin(egui::Margin::symmetric(11, 12))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing = vec2(6.0, 8.0);
            content(ui);
        });
}

fn diamond(ui: &mut Ui, selected: bool, tip: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(23.0, 20.0), egui::Sense::click());
    let c = rect.center();
    let color = if selected || response.hovered() {
        theme::banana()
    } else {
        theme::text_dim()
    };
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            c + vec2(0.0, -5.5),
            c + vec2(5.5, 0.0),
            c + vec2(0.0, 5.5),
            c + vec2(-5.5, 0.0),
        ],
        if selected {
            color
        } else {
            Color32::TRANSPARENT
        },
        egui::Stroke::new(1.0, color),
    ));
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), tip));
    response.on_hover_text(tip).clicked()
}

fn reset_button(ui: &mut Ui) -> bool {
    crate::widgets::icon_button(ui, crate::icons::Icon::Undo, tr("Reset")).clicked()
}

fn field(ui: &mut Ui, value: &mut f32, width: f32, units: &str) -> bool {
    ui.add_sized(
        [width.max(36.0), 22.0],
        crate::fmt::drag(value, 2, units).speed(0.3),
    )
    .changed()
}

fn transform_section(
    ui: &mut Ui,
    clip: &mut Clip,
    at: Tick,
    local: Tick,
    mut place: PlaceIn,
    preferences: (bv_timeline::fx::KeyCurve, f32),
) {
    let original = clip.box_at(at, place.media, place.media_aspect);
    let mut next = original;
    let mut transform = pancrop::transform_from_key(&original, &place);
    let mut enabled = !clip.visual.transform_bypass;
    let mut key = false;
    let mut reset = false;
    let keyed = (clip.visual.transform_keyed || clip.pan_crop.keys.len() > 1)
        && clip.pan_crop.keys.iter().any(|point| point.at == local);
    let open = header(ui, tr("Transform"), &mut enabled, true, |ui| {
        key |= diamond(ui, keyed, tr("Add Transform Keyframe"));
        reset = reset_button(ui);
    });
    clip.visual.transform_bypass = !enabled;
    if reset {
        reset_transform(clip);
        return;
    }

    if open {
        let mut changed = false;
        let mut independent = false;
        body(ui, |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                let scale_id = ui.id().with(("linked-scale", clip.id.0));
                let mut linked = ui.data(|d| d.get_temp(scale_id)).unwrap_or(true);
                ui.label(tr("Scale"));
                ui.horizontal(|ui| {
                    if crate::widgets::icon_toggle(
                        ui,
                        crate::icons::Icon::Lock,
                        linked,
                        tr("Link Scale X and Y"),
                    )
                    .clicked()
                    {
                        linked = !linked;
                    }
                    ui.vertical(|ui| {
                        let previous = [transform.scale, transform.scale * transform.stretch];
                        let mut scale = previous.map(|v| 100.0 * v);
                        for (index, value) in scale.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(if index == 0 { "X" } else { "Y" })
                                        .color(theme::text_dim()),
                                );
                                let width = ui.available_width() - 31.0;
                                if field(ui, value, width, " %") {
                                    let edited = (*value / 100.0).clamp(0.01, 40.0);
                                    if linked {
                                        transform.scale = if index == 0 {
                                            edited
                                        } else {
                                            edited / transform.stretch.max(0.0001)
                                        };
                                    } else if index == 0 {
                                        transform.scale = edited;
                                        transform.stretch = previous[1] / edited;
                                    } else {
                                        transform.stretch = edited / transform.scale.max(0.0001);
                                    }
                                    changed = true;
                                    independent = !linked;
                                }
                                key |= diamond(
                                    ui,
                                    keyed,
                                    if index == 0 {
                                        tr("Add Scale X Keyframe")
                                    } else {
                                        tr("Add Scale Y Keyframe")
                                    },
                                );
                            });
                        }
                    });
                });
                ui.data_mut(|d| d.insert_temp(scale_id, linked));
                ui.add_space(9.0);
                ui.horizontal(|ui| {
                    ui.label(tr("Position"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        key |= diamond(ui, keyed, tr("Add Position Keyframe"));
                    });
                });
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - 50.0) / 2.0;
                    for (index, value) in
                        [&mut transform.x, &mut transform.y].into_iter().enumerate()
                    {
                        let frame = place.frame[index] as f32;
                        let mut pixels = *value * frame;
                        ui.label(
                            RichText::new(if index == 0 { "X" } else { "Y" })
                                .color(theme::text_dim()),
                        );
                        if field(ui, &mut pixels, width, " px") {
                            *value = (pixels / frame.max(1.0)).clamp(-10.0, 10.0);
                            changed = true;
                        }
                    }
                });
                ui.add_space(9.0);
                ui.horizontal(|ui| {
                    ui.label(tr("Rotate"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        key |= diamond(ui, keyed, tr("Add Rotation Keyframe"));
                        if reset_button(ui) {
                            transform.rotation_deg = 0.0;
                            changed = true;
                        }
                    });
                });
                ui.horizontal(|ui| {
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(23.0, 23.0), egui::Sense::drag());
                    let angle = transform.rotation_deg.to_radians();
                    let center = rect.center();
                    ui.painter().circle_filled(center, 10.0, theme::knob());
                    ui.painter().circle_filled(
                        center + vec2(angle.sin(), -angle.cos()) * 6.0,
                        3.0,
                        theme::text_strong(),
                    );
                    if response.dragged() {
                        transform.rotation_deg += ui.input(|i| i.pointer.delta().x);
                        changed = true;
                    }
                    let width = ui.available_width();
                    changed |= field(ui, &mut transform.rotation_deg, width, "°");
                });
                if changed {
                    if independent && place.keep_aspect {
                        let old_place = place;
                        clip.pan_crop.keep_aspect = false;
                        place.keep_aspect = false;
                        clip.pan_crop.ensure_size(place.media);
                        for point in &mut clip.pan_crop.keys {
                            let tf = pancrop::transform_from_key(point, &old_place);
                            *point = applied_transform(&tf, &place, *point);
                        }
                    }
                    next = applied_transform(&transform, &place, original);
                }
                ui.add_space(9.0);
                if let Some(orientation) = orientation_strip(ui, &next) {
                    next = orientation;
                }
            });
        });
    }

    if key || next != original {
        let edit = if key || clip.visual.transform_keyed || clip.pan_crop.keys.len() > 1 {
            local
        } else {
            clip.pan_crop
                .keys
                .first()
                .map_or(Tick::ZERO, |point| point.at)
        };
        if key {
            clip.visual.transform_keyed = true;
        }
        clip.pan_crop.set_at(
            edit,
            next,
            place.media,
            (preferences.0, f64::from(preferences.1 / 100.0)),
        );
    }
}

fn applied_transform(tf: &Transform, place: &PlaceIn, old: PanCropKey) -> PanCropKey {
    let mut next = pancrop::key_from_transform(tf, place);
    next.pivot = std::array::from_fn(|i| next.center[i] + old.pivot[i] - old.center[i]);
    PanCropKey {
        at: old.at,
        curve: old.curve,
        smoothness: old.smoothness,
        ..next
    }
}

fn compositing_section(
    ui: &mut Ui,
    clip: &mut Clip,
    local: Tick,
    curve: bv_timeline::fx::KeyCurve,
) {
    let mut enabled = !clip.visual.compositing_bypass;
    let mut add_key = false;
    let envelope = clip.envelope(OPACITY);
    let keyed = envelope.is_some_and(|env| env.points.iter().any(|point| point.at == local));
    let mut opacity = envelope.map_or(clip.level, |env| env.value_at(local)) * 100.0;
    let open = header(ui, tr("Compositing"), &mut enabled, true, |ui| {
        add_key |= diamond(ui, keyed, tr("Add Opacity Keyframe"));
    });
    clip.visual.compositing_bypass = !enabled;
    let mut changed = false;
    if open {
        body(ui, |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                ui.label(tr("Blend Mode"));
                egui::ComboBox::from_id_salt("clip-blend-mode")
                    .width(ui.available_width())
                    .selected_text(blend_name(
                        clip.visual.blend.unwrap_or(CompositeMode::SourceAlpha),
                    ))
                    .show_ui(ui, |ui| {
                        for mode in CompositeMode::ALL {
                            ui.selectable_value(
                                &mut clip.visual.blend,
                                Some(mode),
                                blend_name(mode),
                            );
                        }
                    });
                ui.add_space(9.0);
                ui.horizontal(|ui| {
                    ui.label(tr("Opacity"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        add_key |= diamond(ui, keyed, tr("Add Opacity Keyframe"));
                    });
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().slider_width = (ui.available_width() - 78.0).max(60.0);
                    changed |= ui
                        .add(egui::Slider::new(&mut opacity, 0.0..=100.0).show_value(false))
                        .changed();
                    changed |= field(ui, &mut opacity, 65.0, " %");
                });
            });
        });
    }
    if add_key || changed {
        let value = (opacity / 100.0).clamp(0.0, 1.0);
        if add_key || clip.envelope(OPACITY).is_some() {
            if clip.envelope(OPACITY).is_none() {
                let mut env = Envelope::with_curve(OPACITY, curve);
                env.points[0].value = clip.level;
                clip.envelopes.push(env);
            }
            if let Some(env) = clip.envelopes.iter_mut().find(|env| env.kind == OPACITY) {
                match env.points.binary_search_by_key(&local, |point| point.at) {
                    Ok(index) => env.points[index].value = value,
                    Err(index) => env.points.insert(
                        index,
                        bv_timeline::envelope::EnvelopePoint {
                            at: local,
                            value,
                            curve,
                        },
                    ),
                }
            }
        } else {
            clip.level = value;
        }
    }
}

fn blend_name(mode: CompositeMode) -> &'static str {
    if mode == CompositeMode::SourceAlpha {
        tr("Normal")
    } else {
        tr(mode.name())
    }
}

fn extras(ui: &mut Ui, clip: &mut Clip) {
    let v = &mut clip.visual;
    if header(ui, tr("Background"), &mut v.background_on, false, |_| {}) {
        body(ui, |ui| {
            ui.add_enabled_ui(v.background_on, |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr("Color"));
                    ui.color_edit_button_srgba_unmultiplied(&mut v.background);
                });
            });
        });
    }
    if header(ui, tr("Auto Enhance"), &mut v.enhance_on, false, |_| {}) {
        body(ui, |ui| {
            ui.add_enabled_ui(v.enhance_on, |ui| {
                let mut strength = v.enhance_strength * 100.0;
                if value_slider(ui, tr("Strength"), &mut strength, 0.0..=100.0, 0, " %") {
                    v.enhance_strength = strength / 100.0;
                }
            });
        });
    }
    if header(ui, tr("Drop Shadow"), &mut v.shadow_on, false, |_| {}) {
        body(ui, |ui| {
            ui.add_enabled_ui(v.shadow_on, |ui| {
                let look = &mut v.shadow;
                for (name, value, range) in [
                    (tr("Opacity"), &mut look.intensity, 0.0..=100.0),
                    (tr("Blur"), &mut look.blur, 0.0..=100.0),
                    (tr("Offset X"), &mut look.x, -50.0..=50.0),
                    (tr("Offset Y"), &mut look.y, -50.0..=50.0),
                ] {
                    let mut percent = *value * 100.0;
                    if value_slider(ui, name, &mut percent, range, 1, " %") {
                        *value = percent / 100.0;
                    }
                }
                ui.horizontal(|ui| {
                    ui.label(tr("Color"));
                    ui.color_edit_button_rgba_unmultiplied(&mut look.color);
                });
            });
        });
    }
}

fn keyframe_panel(ui: &mut Ui, app: &mut BananaApp, clip: &mut Clip) {
    ui.strong(tr("Keyframe Panel"));
    if ui.button(tr("Open Pan/Crop…")).clicked() {
        app.clip_windows.pan_crop = Some(clip.id);
    }
    ui.label(tr("Transform"));
    let animated = clip.visual.transform_keyed || clip.pan_crop.keys.len() > 1;
    if animated {
        let points = clip.pan_crop.keys.iter().map(|p| p.at).collect::<Vec<_>>();
        if let Some(index) = keyframe_rows(ui, app, clip.start, &points, !clip.locked) {
            if let Some(point) = points.get(index) {
                clip.pan_crop.remove_key(*point);
                clip.visual.transform_keyed = !clip.pan_crop.keys.is_empty();
            }
        }
    } else {
        ui.label(RichText::new(tr("No keyframes")).color(theme::text_dim()));
    }
    ui.label(tr("Opacity"));
    if let Some(env) = clip.envelopes.iter_mut().find(|e| e.kind == OPACITY) {
        let points = env.points.iter().map(|p| p.at).collect::<Vec<_>>();
        if let Some(index) = keyframe_rows(ui, app, clip.start, &points, !clip.locked) {
            let removed = env.points.remove(index);
            if env.points.is_empty() {
                clip.level = removed.value;
                clip.envelopes.retain(|e| e.kind != OPACITY);
            }
        }
    } else {
        ui.label(RichText::new(tr("No keyframes")).color(theme::text_dim()));
    }
}

fn keyframe_rows(
    ui: &mut Ui,
    app: &mut BananaApp,
    start: Tick,
    points: &[Tick],
    editable: bool,
) -> Option<usize> {
    let rate = app.project().settings.frame_rate;
    let mut remove = None;
    for (index, time) in points.iter().copied().enumerate() {
        ui.horizontal(|ui| {
            if ui
                .selectable_label(app.cursor == start + time, timecode::format(time, rate))
                .clicked()
            {
                app.seek(start + time);
            }
            if ui
                .add_enabled(editable, egui::Button::new(tr("Delete")))
                .clicked()
            {
                remove = Some(index);
            }
        });
    }
    remove
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::accesskit::Role;
    use egui_kittest::{Harness, kittest::Queryable};

    #[test]
    fn section_visibility_does_not_change_its_enabled_state() {
        let mut harness = Harness::new_ui_state(
            |ui, enabled| {
                if header(ui, "Transform", enabled, true, |_| {}) {
                    ui.label("Scale X");
                }
            },
            true,
        );
        harness.get_by_label("Scale X");
        harness
            .get_by_role_and_label(Role::Button, "Transform")
            .click();
        harness.run();
        assert!(*harness.state());
        assert!(harness.query_by_label("Scale X").is_none());
        harness
            .get_by_role_and_label(Role::CheckBox, "Transform")
            .click();
        harness.run();
        assert!(!*harness.state());
        assert!(harness.query_by_label("Scale X").is_none());
        harness
            .get_by_role_and_label(Role::Button, "Transform")
            .click();
        harness.run();
        harness.get_by_label("Scale X");
        assert!(!*harness.state());
    }
}
