//! Selection-specific controls for timeline clips.

use crate::i18n::tr;
use bv_core::{FrameRate, Tick, timecode};
use bv_timeline::mask::{MaskFeather, MaskPath, PanCropMask};
use bv_timeline::{
    AnimationPreset, Clip, ClipAnimation, ClipId, Effect, EffectKind, FadeCurve, Generator,
    MediaSource, ParamKind, Project, TrackKind, Transform, Transition, VideoStreamInfo,
};

use crate::app::BananaApp;
use crate::dock::Tab;
use crate::egui::{self, Color32, RichText};
use crate::theme;

const CURVES: [(FadeCurve, &str); 5] = [
    (FadeCurve::Linear, "Linear"),
    (FadeCurve::Fast, "Fast"),
    (FadeCurve::Slow, "Slow"),
    (FadeCurve::Smooth, "Smooth"),
    (FadeCurve::Sharp, "Sharp"),
];

fn curve_name(c: FadeCurve) -> &'static str {
    CURVES.iter().find(|x| x.0 == c).map_or("?", |x| tr(x.1))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Visual,
    Audio,
    Speed,
    Animation,
    Adjust,
    Tracking,
    Effects,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VisualPage {
    Basic,
    RemoveBackground,
    Mask,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AnimationPage {
    In,
    Out,
}

/// A vector chevron rather than a font-dependent symbol.
/// The button scrolls the tab strip, never changes the active properties page.
fn tab_chevron(ui: &mut egui::Ui, forward: bool, enabled: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(25.0, 30.0),
        if enabled { egui::Sense::click() } else { egui::Sense::hover() },
    );
    if theme::premium() && enabled {
        theme::paint_premium_button(ui.painter(), rect, false, response.hovered(), false);
    } else if response.hovered() && enabled {
        ui.painter().rect_filled(rect, 5.0, theme::hover());
    }
    let c = rect.center();
    let color = if enabled { theme::text() } else { theme::text_disabled() };
    let x = if forward { 1.0 } else { -1.0 };
    let stroke = egui::Stroke::new(1.8, color);
    ui.painter().line_segment(
        [c + egui::vec2(-3.0 * x, -6.0), c + egui::vec2(3.0 * x, 0.0)],
        stroke,
    );
    ui.painter().line_segment(
        [c + egui::vec2(3.0 * x, 0.0), c + egui::vec2(-3.0 * x, 6.0)],
        stroke,
    );
    let tip = if forward { tr("More tabs to the right") } else { tr("More tabs to the left") };
    response.on_hover_text(tip).clicked()
}

/// Keep the properties navigation on exactly one row. When the panel is
/// narrower than the tabs, arrow controls reveal the remaining tabs.
/// The chosen page stays selected; changing panel width changes no settings.
fn tabs<T: Copy + Eq + Send + Sync + 'static>(
    ui: &mut egui::Ui,
    salt: &str,
    items: &[(T, &str)],
) -> T {
    let key = ui.id().with(salt);
    let mut active = ui
        .data(|d| d.get_temp::<T>(key))
        .filter(|active| items.iter().any(|(item, _)| item == active))
        .unwrap_or(items[0].0);
    egui::Frame::new()
        .fill(theme::panel_dark())
        .corner_radius(if theme::premium() { 10.0 } else { 4.0 })
        .inner_margin(egui::Margin::same(if theme::premium() { 6 } else { 4 }))
        .show(ui, |ui| {
            let font = egui::FontId::proportional(13.0);
            let widths: Vec<f32> = items
                .iter()
                .map(|(_, label)| {
                    ui.painter()
                        .layout_no_wrap(label.to_string(), font.clone(), theme::text())
                        .size()
                        .x
                        + 24.0
                })
                .collect();
            let gap = 4.0;
            let total = widths.iter().sum::<f32>()
                + gap * (widths.len().saturating_sub(1)) as f32;
            let available = ui.available_width().max(1.0);
            let overflowing = total > available;
            let budget = if overflowing { (available - 60.0).max(20.0) } else { available };
            let start_key = key.with("first-visible");
            let last_key = key.with("last-selection");
            let mut first = ui
                .data(|d| d.get_temp::<usize>(start_key))
                .unwrap_or(0)
                .min(items.len().saturating_sub(1));
            if !overflowing {
                first = 0;
            } else {
                let chosen = items.iter().position(|(tab, _)| *tab == active).unwrap_or(0);
                let selection_changed = ui.data(|d| d.get_temp::<T>(last_key)) != Some(active);
                if selection_changed {
                    if chosen < first {
                        first = chosen;
                    }
                    // Fit the active tab in the view when the selected page
                    // changes, but not after a user clicks an overflow arrow.
                    while first < chosen
                        && widths[first..=chosen].iter().sum::<f32>()
                            + gap * (chosen - first) as f32
                            > budget
                    {
                        first += 1;
                    }
                }
            }
            let mut end = first;
            let mut used = 0.0;
            while end < items.len() {
                let w = widths[end] + if end > first { gap } else { 0.0 };
                if end > first && used + w > budget {
                    break;
                }
                used += w;
                end += 1;
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                if overflowing && tab_chevron(ui, false, first > 0) {
                    first = first.saturating_sub(1);
                }
                for i in first..end {
                    let (tab, label) = items[i];
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(widths[i].min(budget), 30.0),
                        egui::Sense::click(),
                    );
                    let selected = tab == active;
                    if theme::premium() {
                        theme::paint_premium_button(
                            ui.painter(), rect, selected, response.hovered(), false,
                        );
                    } else if selected {
                        ui.painter().rect_filled(rect, 4.0, theme::selection());
                    } else if response.hovered() {
                        ui.painter().rect_filled(rect, 4.0, theme::hover());
                    }
                    ui.painter().with_clip_rect(rect).text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        label,
                        font.clone(),
                        if selected && theme::premium() { Color32::WHITE }
                        else if selected { theme::text_strong() } else { theme::text() },
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::SelectableLabel, true, selected, label,
                        )
                    });
                    if response.clicked() {
                        active = tab;
                    }
                }
                if overflowing && tab_chevron(ui, true, end < items.len()) {
                    first = (first + 1).min(items.len() - 1);
                }
            });
            ui.data_mut(|d| d.insert_temp(start_key, first));
            ui.data_mut(|d| d.insert_temp(last_key, active));
        });
    ui.data_mut(|d| d.insert_temp(key, active));
    active
}

pub fn ui(ui: &mut egui::Ui, app: &mut BananaApp) {
    let Some(id) = app.primary_clip() else {
        super::hint(
            ui,
            tr("Select a clip on the timeline to see and edit its properties."),
        );
        return;
    };
    let project = app.project().clone();
    let (Some(clip), Some(track)) = (project.clip(id), project.clip_track(id)) else {
        return;
    };
    if track.locked {
        super::hint(ui, tr("This track is locked."));
        return;
    }

    let rate = project.settings.frame_rate;
    let media = project.media(clip.media);
    let media_name = media.map_or_else(String::new, crate::i18n::media_name);
    let is_visual = track.kind == TrackKind::Video;
    let still = media.is_some_and(|m| m.still);
    let is_title = is_visual
        && media.is_some_and(|m| {
            matches!(&m.source, MediaSource::Generated(Generator::Plugin(fx))
                if fx.id == bv_engine::plugins::TITLES_ID)
        });
    let members = project.group_members(id);
    let group_locked = members
        .iter()
        .any(|&member| project.clip_track(member).is_some_and(|track| track.locked));
    let linked_audio = if is_visual {
        members.into_iter().find_map(|member| {
            let track = project.clip_track(member)?;
            (track.kind == TrackKind::Audio && !track.locked)
                .then(|| project.clip(member))
                .flatten()
        })
    } else {
        None
    };

    let mut pages = Vec::new();
    let visual_name = if is_title {
        tr("Text")
    } else if still {
        tr("Image")
    } else {
        tr("Video")
    };
    if is_visual {
        pages.push((Page::Visual, visual_name));
    }
    if !is_visual || linked_audio.is_some() {
        let label = if is_visual { tr("Audio") } else { tr("Basic") };
        pages.push((Page::Audio, label));
    }
    if !is_visual || (!still && !is_title) {
        pages.push((Page::Speed, tr("Speed")));
    }
    if is_visual {
        pages.extend([
            (Page::Animation, tr("Animation")),
            (Page::Adjust, tr("Adjust")),
            (Page::Tracking, tr("Motion Tracking")),
        ]);
    }
    pages.push((Page::Effects, tr("Effects")));
    let kind = (is_visual, still, is_title);

    let at = if app.cursor >= clip.start && app.cursor < clip.end() {
        app.cursor
    } else {
        clip.start
    };
    let place = project.clip_place_in(id);
    let shown = place.map(|place| clip.box_at(at, place.media, place.media_aspect));
    let framing = place
        .zip(shown)
        .map_or_else(Transform::default, |(place, key)| {
            bv_timeline::pancrop::transform_from_key(&key, &place)
        });

    let mut c = clip.clone();
    let mut audio = linked_audio.cloned();
    let mut tf = framing;
    let mut conform = false;
    let mut speed = None;
    let mut preserve_linked_pitch = None;
    let key = ui.id().with(("clip-properties", id.0));

    let mut frame = egui::Frame::new()
        .inner_margin(egui::Margin::same(12));
    if theme::premium() {
        frame = frame
            .fill(theme::panel())
            .stroke(egui::Stroke::new(1.0, theme::separator()))
            .corner_radius(10.0);
    }
    frame.show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            ui.add(egui::Label::new(RichText::new(&media_name).strong()).truncate());
            let page = ui.push_id(kind, |ui| tabs(ui, "pages", &pages)).inner;
            let visual_page = (page == Page::Visual).then(|| {
                if is_title {
                    tabs(
                        ui,
                        "text",
                        &[(VisualPage::Basic, tr("Basic")), (VisualPage::Mask, tr("Mask"))],
                    )
                } else {
                    tabs(
                        ui,
                        "visual",
                        &[
                            (VisualPage::Basic, tr("Basic")),
                            (VisualPage::RemoveBackground, tr("Remove BG")),
                            (VisualPage::Mask, tr("Mask")),
                        ],
                    )
                }
            });
            let animation_page = (page == Page::Animation).then(|| {
                tabs(
                    ui,
                    "animation",
                    &[(AnimationPage::In, tr("In")), (AnimationPage::Out, tr("Out"))],
                )
            });

            egui::ScrollArea::vertical()
                .id_salt((
                    id.0,
                    page as u8,
                    visual_page.map(|p| p as u8),
                    animation_page.map(|p| p as u8),
                ))
                .auto_shrink([false, false])
                .show(ui, |ui| match page {
                    Page::Visual => match visual_page {
                        Some(VisualPage::RemoveBackground) => {
                            chroma_key(ui, app, &project, &mut c, id);
                        }
                        Some(VisualPage::Mask) => {
                            mask(ui, app, &mut c, media.and_then(|m| m.video), at - clip.start);
                        }
                        _ => {
                            if is_title && ui.button(tr("Edit Text")).clicked() {
                                app.open_generated_media(id);
                            }
                            pan_crop(ui, &mut tf);
                            if ui.button(tr("Open Pan/Crop…")).clicked() {
                                app.clip_windows.pan_crop = Some(id);
                            }
                            if clip.pan_crop.keys.len() > 1 {
                                super::hint(ui, tr("Animated: edits the keyframe at the cursor."));
                            }
                            section(ui, tr("Blend"));
                            let mut opacity = c.level * 100.0;
                            if value_slider(ui, tr("Opacity"), &mut opacity, 0.0..=100.0, 0, " %") {
                                c.level = opacity / 100.0;
                            }
                            egui::CollapsingHeader::new(tr("Clip Settings")).show(ui, |ui| {
                                clip_settings(ui, &mut c, rate);
                                if !still && !is_title {
                                    conform = video_settings(
                                        ui,
                                        &mut c,
                                        media.and_then(|m| m.video),
                                        rate,
                                        group_locked,
                                    );
                                }
                            });
                        }
                    },
                    Page::Audio => {
                        let target = audio.as_mut().unwrap_or(&mut c);
                        audio_controls(ui, target, rate);
                        egui::CollapsingHeader::new(tr("Clip Settings"))
                            .show(ui, |ui| clip_settings(ui, target, rate));
                    }
                    Page::Speed => {
                        let mut playback = c.rate as f32;
                        ui.add_enabled_ui(!group_locked, |ui| {
                            if value_slider(
                                ui,
                                tr("Playback rate"),
                                &mut playback,
                                0.05..=20.0,
                                2,
                                "×",
                            ) {
                                speed = Some(f64::from(playback));
                            }
                        });
                        ui.label(trf!("Duration: {}", timecode::format(c.duration, rate)));
                        if let Some(linked) = audio.as_ref() {
                            let mut preserve = linked.preserve_pitch;
                            if crate::widgets::checkbox(
                                ui,
                                &mut preserve,
                                tr("Preserve pitch when stretched"),
                            )
                            .changed()
                            {
                                preserve_linked_pitch = Some((linked.id, preserve));
                            }
                        } else if !is_visual {
                            crate::widgets::checkbox(
                                ui,
                                &mut c.preserve_pitch,
                                tr("Preserve pitch when stretched"),
                            );
                        }
                    }
                    Page::Animation => {
                        let animation = if animation_page == Some(AnimationPage::Out) {
                            &mut c.animation_out
                        } else {
                            &mut c.animation_in
                        };
                        animation_tiles(ui, animation, c.duration);
                        section(ui, tr("Fades and transition"));
                        fades(ui, &mut c, rate);
                        ui.label(tr("Transition in"));
                        transition_choice(ui, &mut c);
                        transition_params(ui, app, &project, id, &mut c);
                    }
                    Page::Adjust => super::color::inspector(ui, app, id),
                    Page::Tracking => {
                        section(ui, tr("Motion Tracking"));
                        if ui.button(tr("Motion Tracking")).clicked() {
                            app.open_motion_tracking(Some(id));
                        }
                    }
                    Page::Effects => effects_chain(ui, &mut c, app, !is_visual),
                });
        });

    if conform {
        app.edit(|p| p.conform_to_project_rate(id, false));
        return;
    }
    if let (Some(edited), Some(before)) = (audio, linked_audio) {
        store_clip(ui, app, before, edited);
    }
    store_clip(ui, app, clip, c);
    if let Some(speed) = speed {
        app.modify_merged(key.with("speed"), |p| {
            let _ = p.set_clip_rate(id, speed);
        });
    }
    if let Some((id, preserve)) = preserve_linked_pitch {
        app.modify(|p| {
            if let Some(c) = p.clip_mut(id) {
                c.preserve_pitch = preserve;
            }
        });
    }

    if tf != framing
        && let (Some(place), Some(was)) = (place, shown)
    {
        let mut now = bv_timeline::pancrop::key_from_transform(&tf, &place);
        now.pivot = [
            now.center[0] + was.pivot[0] - was.center[0],
            now.center[1] + was.pivot[1] - was.center[1],
        ];
        let new = app.settings.prefs.new_motion_keys();
        app.modify_merged(key.with("transform"), |p| {
            let _ = p.set_clip_box_at(id, at, now, new);
        });
    }
}

fn store_clip(ui: &egui::Ui, app: &mut BananaApp, before: &Clip, after: Clip) {
    if after == *before {
        return;
    }
    let id = before.id;
    let key = ui.id().with(("clipe", id.0));
    let mut same = after.clone();
    for (effect, old) in same.effects.iter_mut().zip(&before.effects) {
        effect.params.clone_from(&old.params);
    }
    if same == *before {
        let chain = move |p: &Project| p.clip(id).map(|c| c.effects.clone());
        let set = move |p: &mut Project, effects| {
            if let Some(c) = p.clip_mut(id) {
                c.effects = effects;
            }
        };
        app.set_fx_params(key, after.effects, chain, set);
    } else {
        apply_clip(app, key, before, after);
    }
}

pub(super) fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(4.0);
    ui.separator();
    ui.add_space(3.0);
    ui.strong(title);
}

pub(super) fn value_slider(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    decimals: usize,
    unit: &str,
) -> bool {
    ui.label(label);
    ui.push_id(label, |ui| {
        ui.horizontal(|ui| {
            let field_width = 64.0;
            ui.spacing_mut().slider_width =
                (ui.available_width() - field_width - ui.spacing().item_spacing.x).max(28.0);
            let slider = ui.add(
                crate::fmt::slider(value, range.clone(), decimals, unit)
                    .clamping(egui::SliderClamping::Edits)
                    .show_value(false),
            );
            let field = ui.add_sized(
                [field_width, ui.spacing().interact_size.y],
                crate::fmt::drag(value, decimals, unit)
                    .range(range)
                    .clamp_existing_to_range(false),
            );
            slider.changed() || field.changed()
        }).inner
    }).inner
}

fn transform(ui: &mut egui::Ui, tf: &mut Transform) {
    ui.horizontal(|ui| {
        ui.strong(tr("Transform"));
        if ui.small_button(tr("Reset")).clicked() {
            *tf = Transform::default();
        }
    });
    percent(ui, tr("Scale"), &mut tf.scale, 5.0..=400.0);
    ui.label(tr("Position"));
    ui.horizontal_wrapped(|ui| {
        for (axis, value) in [("X", &mut tf.x), ("Y", &mut tf.y)] {
            let mut shown = *value * 100.0;
            if ui.add_sized(
                [78.0, ui.spacing().interact_size.y],
                crate::fmt::drag(&mut shown, 1, " %")
                    .range(-100.0..=100.0)
                    .prefix(format!("{axis}  ")),
            ).changed() {
                *value = shown / 100.0;
            }
        }
    });
    value_slider(ui, tr("Rotation"), &mut tf.rotation_deg, -180.0..=180.0, 1, "°");
    egui::CollapsingHeader::new(tr("Crop")).show(ui, |ui| {
        percent(ui, tr("Crop left"), &mut tf.crop_left, 0.0..=95.0);
        percent(ui, tr("Crop right"), &mut tf.crop_right, 0.0..=95.0);
        percent(ui, tr("Crop top"), &mut tf.crop_top, 0.0..=95.0);
        percent(ui, tr("Crop bottom"), &mut tf.crop_bottom, 0.0..=95.0);
    });
}

fn percent(ui: &mut egui::Ui, label: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>) {
    let mut shown = *value * 100.0;
    if value_slider(ui, label, &mut shown, range, 1, " %") {
        *value = shown / 100.0;
    }
}

fn clip_settings(ui: &mut egui::Ui, c: &mut Clip, rate: FrameRate) {
    ui.label(trf!(
        "Start {}   Length {}   Media from {}",
        timecode::format(c.start, rate),
        timecode::format(c.duration, rate),
        timecode::format(c.source_in, rate)
    ));
    crate::widgets::checkbox(ui, &mut c.muted, tr("Disabled"));
    crate::widgets::checkbox(ui, &mut c.locked, tr("Locked"));
}

fn video_settings(
    ui: &mut egui::Ui,
    c: &mut Clip,
    video: Option<VideoStreamInfo>,
    rate: FrameRate,
    group_locked: bool,
) -> bool {
    if crate::widgets::checkbox(ui, &mut c.all_frames, tr("Trim event to include all frames"))
        .changed()
        && c.all_frames
    {
        c.looped = false;
        c.hold_last = false;
    }
    crate::widgets::checkbox(ui, &mut c.reduce_flicker, tr("Reduce interlace flicker"));
    value_slider(ui, tr("Undersample rate:"), &mut c.undersample, 0.01..=1.0, 3, "");
    ui.add_enabled(
        !group_locked && video.is_some_and(|v| v.frame_rate != rate),
        egui::Button::new(tr("Conform to Project Frame Rate")),
    ).clicked()
}

fn audio_controls(ui: &mut egui::Ui, c: &mut Clip, rate: FrameRate) {
    section(ui, tr("Basic"));
    let mut db = bv_mixer_gain_db(c.level);
    if value_slider(ui, tr("Volume"), &mut db, -60.0..=12.0, 1, " dB") {
        c.level = if db <= -59.9 { 0.0 } else { 10f32.powf(db / 20.0) };
    }
    fades(ui, c, rate);
    crate::widgets::checkbox(ui, &mut c.auto_normalize, tr("Auto Normalize"));
    section(ui, tr("Pitch"));
    value_slider(ui, tr("Pitch change (semitones):"), &mut c.pitch, -48.0..=48.0, 2, "");
}

fn fades(ui: &mut egui::Ui, c: &mut Clip, rate: FrameRate) {
    let max = c.duration.as_seconds_f64().min(10.0) as f32;
    for (label, fade, curve) in [
        (tr("Fade in"), &mut c.fade_in, &mut c.fade_in_curve),
        (tr("Fade out"), &mut c.fade_out, &mut c.fade_out_curve),
    ] {
        let mut seconds = fade.as_seconds_f64() as f32;
        if value_slider(ui, label, &mut seconds, 0.0..=max, 2, " s") {
            *fade = rate.snap_nearest(Tick::from_seconds_f64(f64::from(seconds)));
        }
        egui::ComboBox::from_id_salt(label)
            .selected_text(curve_name(*curve))
            .show_ui(ui, |ui| {
                for (cv, name) in CURVES {
                    ui.selectable_value(curve, cv, tr(name));
                }
            });
    }
    c.fade_in = c.fade_in.min(c.duration);
    c.fade_out = c.fade_out.min(c.duration - c.fade_in);
}

fn animation_tiles(ui: &mut egui::Ui, animation: &mut ClipAnimation, duration: Tick) {
    let previous = animation.preset;
    ui.horizontal_wrapped(|ui| {
        for (preset, label) in [
            (AnimationPreset::None, tr("None")),
            (AnimationPreset::Fade, tr("Fade")),
            (AnimationPreset::SlideUp, tr("Slide Up")),
            (AnimationPreset::SlideDown, tr("Slide Down")),
        ] {
            if animation_tile(ui, preset, label, animation.preset == preset) {
                animation.preset = preset;
            }
        }
    });
    if animation.preset != AnimationPreset::None {
        let max = duration.as_seconds_f64().clamp(0.0, 10.0) as f32;
        if previous == AnimationPreset::None && animation.duration <= Tick::ZERO {
            animation.duration = Tick::from_seconds_f64(f64::from(max.min(0.5)));
        }
        let mut seconds = animation.duration.as_seconds_f64() as f32;
        if value_slider(ui, tr("Duration"), &mut seconds, 0.0..=max, 2, " s") {
            animation.duration = Tick::from_seconds_f64(f64::from(seconds));
        }
    }
}

fn animation_tile(ui: &mut egui::Ui, preset: AnimationPreset, label: &str, selected: bool) -> bool {
    use egui::{Align2, FontId, Rect, Stroke, StrokeKind, vec2};

    let text =
        ui.painter()
            .layout_no_wrap(label.to_owned(), FontId::proportional(12.0), theme::text());
    let size = vec2((text.size().x + 16.0).max(72.0), 59.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, label)
    });
    let painter = ui.painter();
    let accent = if selected {
        theme::banana()
    } else {
        theme::text_strong()
    };
    let fill = if response.hovered() {
        theme::hover()
    } else {
        theme::panel_dark()
    };
    painter.rect_filled(rect, 5.0, fill);
    painter.rect_stroke(
        rect,
        5.0,
        Stroke::new(
            1.0,
            if selected || response.has_focus() {
                accent
            } else {
                theme::separator()
            },
        ),
        StrokeKind::Inside,
    );
    let center = rect.center_top() + vec2(0.0, 21.0);
    let stroke = Stroke::new(1.7, accent);
    match preset {
        AnimationPreset::None => {
            painter.circle_stroke(center, 9.0, stroke);
            painter.line_segment([center + vec2(-6.0, 6.0), center + vec2(6.0, -6.0)], stroke);
        }
        AnimationPreset::Fade => {
            for (x, alpha) in [(-10.0, 0.25), (0.0, 0.55), (10.0, 1.0)] {
                painter.rect_filled(
                    Rect::from_center_size(center + vec2(x, 0.0), vec2(7.0, 16.0)),
                    2.0,
                    accent.gamma_multiply(alpha),
                );
            }
        }
        AnimationPreset::SlideUp | AnimationPreset::SlideDown => {
            let direction = if preset == AnimationPreset::SlideUp {
                -1.0
            } else {
                1.0
            };
            painter.rect_stroke(
                Rect::from_center_size(center + vec2(-5.0, direction * 3.0), vec2(15.0, 12.0)),
                2.0,
                stroke,
                StrokeKind::Inside,
            );
            let tip = center + vec2(9.0, direction * 9.0);
            painter.line_segment([center + vec2(9.0, -direction * 8.0), tip], stroke);
            for x in [-4.0, 4.0] {
                painter.line_segment([tip, tip + vec2(x, -direction * 4.0)], stroke);
            }
        }
    }
    painter.text(
        rect.center_bottom() - vec2(0.0, 9.0),
        Align2::CENTER_BOTTOM,
        label,
        FontId::proportional(12.0),
        theme::text_strong(),
    );
    response.clicked()
}

/// Draw reliable shape previews with egui vectors rather than Unicode
/// glyphs (which display as replacement squares on some Windows fonts).
fn mask_shape_tile(ui: &mut egui::Ui, label: &str, ellipse: bool) -> bool {
    use egui::{Align2, FontId, Rect, Stroke, StrokeKind, vec2};
    let (rect, response) =
        ui.allocate_exact_size(vec2(96.0, 66.0), egui::Sense::click());
    let painter = ui.painter();
    if theme::premium() {
        theme::paint_premium_button(painter, rect, false, response.hovered(), false);
    } else {
        painter.rect_filled(
            rect, 5.0,
            if response.hovered() { theme::hover() } else { theme::panel_dark() },
        );
        painter.rect_stroke(
            rect, 5.0, Stroke::new(1.0, theme::separator()), StrokeKind::Inside,
        );
    }
    let c = rect.center_top() + vec2(0.0, 23.0);
    let stroke = Stroke::new(2.0, theme::text_strong());
    if ellipse {
        let points = (0..=48)
            .map(|i| {
                let angle = i as f32 * std::f32::consts::TAU / 48.0;
                c + vec2(angle.cos() * 17.0, angle.sin() * 11.0)
            })
            .collect::<Vec<_>>();
        painter.add(egui::Shape::line(points, stroke));
    } else {
        painter.rect_stroke(
            Rect::from_center_size(c, vec2(34.0, 22.0)),
            2.0, stroke, StrokeKind::Inside,
        );
    }
    painter.text(
        rect.center_bottom() - vec2(0.0, 7.0),
        Align2::CENTER_BOTTOM,
        label,
        FontId::proportional(12.0),
        theme::text(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label)
    });
    response.clicked()
}

fn mask(
    ui: &mut egui::Ui,
    app: &mut BananaApp,
    c: &mut Clip,
    video: Option<VideoStreamInfo>,
    local: Tick,
) {
    section(ui, tr("Mask"));
    let mut enabled = c.mask.as_ref().is_some_and(|mask| mask.enabled);
    if crate::widgets::checkbox(ui, &mut enabled, tr("Active")).changed() {
        c.mask.get_or_insert_with(PanCropMask::default).enabled = enabled;
    }
    let mut added = None;
    if let Some(video) = video {
        let (w, h) = (f64::from(video.width), f64::from(video.height));
        let a = [w * 0.2, h * 0.2];
        let b = [w * 0.8, h * 0.8];
        ui.horizontal_wrapped(|ui| {
            if mask_shape_tile(ui, tr("Rectangle"), false) {
                added = Some(MaskPath::rectangle(a, b));
            }
            if mask_shape_tile(ui, tr("Ellipse"), true) {
                added = Some(MaskPath::oval(a, b));
            }
        });
    }
    let key = ui.id().with(("mask-path", c.id.0));
    let mut picked = ui.data(|d| d.get_temp::<usize>(key)).unwrap_or(0);
    if let Some(path) = added {
        let mask = c.mask.get_or_insert_with(PanCropMask::default);
        mask.enabled = true;
        picked = mask.paths.len();
        mask.add_path(path);
    }
    let path = c.mask.as_mut().and_then(|mask| {
        if mask.paths.is_empty() {
            return None;
        }
        section(ui, tr("Mask settings"));
        picked = picked.min(mask.paths.len() - 1);
        egui::ComboBox::from_id_salt("mask-path")
            .selected_text(trf!("Mask {}", picked + 1))
            .show_ui(ui, |ui| {
                for i in 0..mask.paths.len() {
                    ui.selectable_value(&mut picked, i, trf!("Mask {}", i + 1));
                }
            });
        ui.data_mut(|d| d.insert_temp(key, picked));
        let units = bv_timeline::pancrop::local_units(local);
        if let Some(mut path) = mask.paths_at_units(units).into_iter().nth(picked) {
            let mut feather = if path.feather == MaskFeather::None {
                0.0
            } else {
                path.feather_pct as f32
            };
            if value_slider(ui, tr("Feather"), &mut feather, 0.0..=100.0, 1, " %") {
                path.feather_pct = f64::from(feather);
                if feather == 0.0 {
                    path.feather = MaskFeather::None;
                } else if path.feather == MaskFeather::None {
                    path.feather = MaskFeather::Both;
                }
                mask.set_look_at(local, picked, &path);
            }
        }
        Some(picked)
    });
    if ui.button(tr("Open Pan/Crop…")).clicked() {
        app.clip_windows.pan_crop = Some(c.id);
        app.clip_windows.mask_row = Some((path, None, path.is_some()));
    }
}

fn chroma_key(ui: &mut egui::Ui, app: &mut BananaApp, project: &Project, c: &mut Clip, id: ClipId) {
    const CHROMA: &str = "org.bananav.ChromaKeyer";
    let index = c.effects.iter().position(|effect| {
        effect.kind == EffectKind::ChromaKey
            || effect.plugin.as_ref().is_some_and(|fx| fx.id == CHROMA)
    });
    let mut enabled = index.is_some_and(|i| c.effects[i].enabled);
    let index = if crate::widgets::checkbox(ui, &mut enabled, tr("Chroma key")).changed() {
        if let Some(i) = index {
            c.effects[i].enabled = enabled;
            Some(i)
        } else {
            let effect = bv_ofx::registry().info(CHROMA).map_or_else(
                || Effect::new(EffectKind::ChromaKey),
                |info| Effect::plugin(crate::ofx::plugin_fx(&info)),
            );
            c.effects.push(effect);
            Some(c.effects.len() - 1)
        }
    } else {
        index
    };
    let Some(i) = index else {
        return;
    };
    ui.add_enabled_ui(enabled, |ui| {
        if let Some(fx) = c.effects[i].plugin.clone() {
            let target = crate::fx_window::FxTarget::Clip(id);
            let key = crate::ofx::EditorKey {
                owner: crate::ofx::EditorOwner::Fx(target),
                index: i,
                id: fx.id.clone(),
            };
            let place = crate::ofx::place_of(target, project, app.cursor);
            if let Some(new) = crate::ofx::params(ui, &mut app.ofx, key, &fx, &place, 1) {
                c.effects[i].plugin = Some(Box::new(new));
            }
        } else {
            params(ui, &mut c.effects[i]);
        }
    });
}

fn transition_params(
    ui: &mut egui::Ui,
    app: &mut BananaApp,
    project: &Project,
    id: ClipId,
    c: &mut Clip,
) {
    if let Some(fx) = c.transition_fx.clone() {
        ui.strong(trf!("Transition: {}", fx.name));
        let key = crate::ofx::EditorKey {
            owner: crate::ofx::EditorOwner::Transition(id),
            index: 0,
            id: fx.id.clone(),
        };
        if let Some(place) = crate::ofx::transition_place(project, id, app.cursor)
            && let Some(new) = crate::ofx::params(ui, &mut app.ofx, key, &fx, &place, 1)
        {
            c.transition_fx = Some(Box::new(new));
        }
    }
}

/// The event's transition: one of BananaV's, or an OFX plug-in's.
fn transition_choice(ui: &mut egui::Ui, c: &mut Clip) {
    let ofx = crate::ofx::transitions();
    let shown = match &c.transition_fx {
        Some(fx) => fx.name.clone(),
        None => tr(c.transition.name()).to_string(),
    };
    egui::ComboBox::from_id_salt("transicao")
        .width(200.0)
        .selected_text(shown)
        .show_ui(ui, |ui| {
            for t in Transition::ALL {
                let on = c.transition_fx.is_none() && c.transition == t;
                if ui.selectable_label(on, tr(t.name())).clicked() {
                    c.transition = t;
                    c.transition_fx = None;
                }
            }
            if !ofx.is_empty() {
                ui.separator();
            }
            for p in &ofx {
                let on = c
                    .transition_fx
                    .as_ref()
                    .is_some_and(|fx| fx.id == p.identifier);
                if ui
                    .selectable_label(on, crate::ofx::plugin_name(p))
                    .clicked()
                    && !on
                {
                    c.transition_fx = Some(Box::new(crate::ofx::plugin_fx(p)));
                }
            }
        });
}

fn bv_mixer_gain_db(g: f32) -> f32 {
    if g <= 1e-4 { -60.0 } else { 20.0 * g.log10() }
}

fn apply_clip(app: &mut BananaApp, key: egui::Id, before: &Clip, c: Clip) {
    let id = c.id;
    let fades_changed = (c.fade_in, c.fade_out) != (before.fade_in, before.fade_out);
    let lock_changed = c.locked != before.locked;
    let (fade_in, fade_out, locked) = (c.fade_in, c.fade_out, c.locked);
    app.modify_merged(key, |p: &mut Project| {
        let group = p.group_members(id);
        if let Some(slot) = p.clip_mut(id) {
            *slot = c;
        }
        if fades_changed || lock_changed {
            for member in group.into_iter().filter(|&member| member != id) {
                if p.clip_track(member).is_some_and(|track| track.locked) {
                    continue;
                }
                if let Some(other) = p.clip_mut(member) {
                    if fades_changed {
                        other.fade_in = fade_in.min(other.duration);
                        other.fade_out = fade_out.min(other.duration - other.fade_in);
                    }
                    if lock_changed {
                        other.locked = locked;
                    }
                }
            }
        }
    });
}

pub(crate) fn pan_crop(ui: &mut egui::Ui, t: &mut Transform) {
    transform(ui, t);
}

fn effects_chain(ui: &mut egui::Ui, c: &mut Clip, app: &mut BananaApp, audio: bool) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(tr("Clip Effects")).strong());
        if !audio && ui.small_button(tr("Catalog…")).clicked() {
            app.show_tab(Tab::VideoFx, false);
        }
    });
    chain_editor(ui, &mut c.effects, audio, "props");
}

/// Editor de uma cadeia de efeitos: adicionar, ligar/desligar, reordenar,
/// remover e ajustar parâmetros. `audio` escolhe o catálogo.
pub(crate) fn chain_editor(ui: &mut egui::Ui, effects: &mut Vec<Effect>, audio: bool, salt: &str) {
    ui.menu_button(tr("+ Add Effect"), |ui| {
        let list: &[EffectKind] = if audio {
            &EffectKind::AUDIO
        } else {
            &EffectKind::VIDEO
        };
        for cat in crate::fx_window::categories(list) {
            ui.label(RichText::new(tr(cat)).small().color(theme::text_dim()));
            for &k in list.iter().filter(|k| k.category() == cat) {
                if ui.button(tr(k.name())).clicked() {
                    effects.push(Effect::new(k));
                    ui.close();
                }
            }
        }
    });
    if effects.is_empty() {
        super::hint(ui, tr("No effects yet."));
        return;
    }
    let mut remove = None;
    let mut swap = None;
    let n = effects.len();
    for (i, e) in effects.iter_mut().enumerate() {
        let title = RichText::new(crate::ofx::fx_name(e)).color(if e.enabled {
            theme::text()
        } else {
            theme::text_dim()
        });
        egui::CollapsingHeader::new(title)
            .id_salt((salt, "efeito", i))
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    crate::widgets::checkbox(ui, &mut e.enabled, tr("Active"));
                    if ui.add_enabled(i > 0, egui::Button::new("↑")).clicked() {
                        swap = Some((i, i - 1));
                    }
                    if ui.add_enabled(i + 1 < n, egui::Button::new("↓")).clicked() {
                        swap = Some((i, i + 1));
                    }
                    if ui.button(tr("Remove")).clicked() {
                        remove = Some(i);
                    }
                    if ui.button(tr("Defaults")).clicked() {
                        *e = e.reset();
                    }
                });
                if e.plugin.is_some() {
                    super::hint(ui, tr("Its settings are in the FX window."));
                } else {
                    params(ui, e);
                }
            });
    }
    if let Some((a, b)) = swap {
        effects.swap(a, b);
    }
    if let Some(i) = remove {
        effects.remove(i);
    }
}

/// Controles genéricos a partir da descrição dos parâmetros.
pub fn params(ui: &mut egui::Ui, e: &mut Effect) {
    let specs = e.kind.params();
    egui::Grid::new(("params", e.kind))
        .num_columns(2)
        .show(ui, |ui| {
            let mut i = 0;
            while i < specs.len() {
                let spec = specs[i];
                group_heading(ui, &spec);
                // Greyed out while a switch it waits on is not as it wants.
                let live = spec.live(|k| e.param(k));
                let label = |ui: &mut egui::Ui| {
                    ui.add_enabled(live, egui::Label::new(tr(spec.name)));
                };
                match spec.kind {
                    ParamKind::Color => {
                        ui.label(tr(spec.name));
                        let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                        let mut c = Color32::from_rgb(
                            to8(e.param(i)),
                            to8(e.param(i + 1)),
                            to8(e.param(i + 2)),
                        );
                        if ui.color_edit_button_srgba(&mut c).changed() {
                            e.set_param(i, c.r() as f32 / 255.0);
                            e.set_param(i + 1, c.g() as f32 / 255.0);
                            e.set_param(i + 2, c.b() as f32 / 255.0);
                        }
                        ui.end_row();
                        i += 3;
                        continue;
                    }
                    ParamKind::Toggle => {
                        label(ui);
                        ui.add_enabled_ui(live, |ui| {
                            let mut on = e.param(i) >= 0.5;
                            if crate::widgets::checkbox(ui, &mut on, "").changed() {
                                e.set_param(i, on as u8 as f32);
                            }
                        });
                    }
                    ParamKind::Slider => {
                        label(ui);
                        ui.add_enabled_ui(live, |ui| {
                            let mut v = e.param(i);
                            let r = param_slider(ui, &spec, &mut v);
                            if r.double_clicked()
                                || crate::widgets::bar_double_clicked(ui, &r, true)
                            {
                                v = spec.default;
                            }
                            if v != e.param(i) {
                                e.set_param(i, v);
                            }
                        });
                    }
                    ParamKind::Choice => {
                        label(ui);
                        ui.add_enabled_ui(live, |ui| {
                            let mut v = e.param(i);
                            let at = (v - spec.min).round().max(0.0) as usize;
                            let shown = spec.options.get(at).map_or("", |o| tr(o));
                            egui::ComboBox::from_id_salt(("param-choice", e.kind, i))
                                .selected_text(shown)
                                .show_ui(ui, |ui| {
                                    for (k, o) in spec.options.iter().enumerate() {
                                        let value = spec.min + k as f32;
                                        ui.selectable_value(&mut v, value, tr(o));
                                    }
                                });
                            if v != e.param(i) {
                                e.set_param(i, v);
                            }
                        });
                    }
                    ParamKind::Data => {
                        i += 1;
                        continue;
                    }
                    ParamKind::Graph => {
                        let points = spec.graph.map_or(0, |g| g.params());
                        label(ui);
                        if let Some(g) = spec.graph {
                            ui.vertical(|ui| graph_editor(ui, e, i, &g));
                        }
                        ui.end_row();
                        i += 1 + points;
                        continue;
                    }
                }
                ui.end_row();
                i += 1;
            }
        });
}

/// A graph's points as its effect holds them: `count` at `at`, then
/// `(x, y)` pairs.
fn graph_points(e: &Effect, at: usize) -> Vec<(f32, f32)> {
    let count = e.param(at).round().max(0.0) as usize;
    (0..count)
        .map(|k| (e.param(at + 1 + 2 * k), e.param(at + 2 + 2 * k)))
        .collect()
}

fn set_graph_points(e: &mut Effect, at: usize, points: &[(f32, f32)]) {
    e.set_param(at, points.len() as f32);
    for (k, (x, y)) in points.iter().enumerate() {
        e.set_param(at + 1 + 2 * k, *x);
        e.set_param(at + 2 + 2 * k, *y);
    }
}

/// A graph parameter's editor: its points joined by straight lines; drag
/// a point to move it (the first and last stay at the ends when pinned),
/// double-click the background to add one, right-click a point to take it
/// away (never below two).
fn graph_editor(ui: &mut egui::Ui, e: &mut Effect, at: usize, g: &bv_timeline::Graph) {
    let width = ui.available_width().clamp(240.0, 520.0);
    let (rect, background) = ui.allocate_exact_size(egui::vec2(width, 160.0), egui::Sense::click());
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals();
    painter.rect_filled(rect, 2.0, visuals.extreme_bg_color);
    let stroke = egui::Stroke::new(1.0, visuals.weak_text_color().gamma_multiply(0.35));
    for k in 1..4 {
        let t = k as f32 / 4.0;
        painter.hline(rect.x_range(), rect.top() + t * rect.height(), stroke);
        painter.vline(rect.left() + t * rect.width(), rect.y_range(), stroke);
    }
    let (x0, x1) = g.x;
    let (y0, y1) = g.y;
    let to_unit_x = |x: f32| {
        if g.log_x && x0 > 0.0 {
            (x.max(x0) / x0).ln() / (x1 / x0).ln()
        } else {
            (x - x0) / (x1 - x0)
        }
    };
    let from_unit_x = |u: f32| {
        if g.log_x && x0 > 0.0 {
            x0 * (x1 / x0).powf(u)
        } else {
            x0 + u * (x1 - x0)
        }
    };
    // The curve drawn a little inside, so the points at its ends show whole.
    let plot = rect.shrink(6.0);
    let to_screen = |(x, y): (f32, f32)| {
        egui::pos2(
            plot.left() + to_unit_x(x).clamp(0.0, 1.0) * plot.width(),
            plot.bottom() - ((y - y0) / (y1 - y0)).clamp(0.0, 1.0) * plot.height(),
        )
    };
    let from_screen = |p: egui::Pos2| {
        let u = ((p.x - plot.left()) / plot.width()).clamp(0.0, 1.0);
        let v = ((plot.bottom() - p.y) / plot.height()).clamp(0.0, 1.0);
        (from_unit_x(u), y0 + v * (y1 - y0))
    };
    let mut points = graph_points(e, at);
    let mut changed = false;
    let line: Vec<egui::Pos2> = points.iter().map(|p| to_screen(*p)).collect();
    painter.add(egui::Shape::line(
        line.clone(),
        egui::Stroke::new(2.0, visuals.selection.bg_fill),
    ));
    let last = points.len().saturating_sub(1);
    let mut remove = None;
    for (k, pos) in line.iter().enumerate() {
        let id = ui.id().with(("graph-point", at, k));
        let r = ui.interact(
            egui::Rect::from_center_size(*pos, egui::vec2(12.0, 12.0)),
            id,
            egui::Sense::click_and_drag(),
        );
        let hot = r.hovered() || r.dragged();
        painter.circle_filled(
            *pos,
            if hot { 5.0 } else { 4.0 },
            visuals.strong_text_color(),
        );
        if r.dragged()
            && let Some(p) = r.interact_pointer_pos()
        {
            let (mut x, y) = from_screen(p);
            // Between its neighbours; the ends stay put when pinned.
            let lo = if k == 0 { x0 } else { points[k - 1].0 };
            let hi = if k == last { x1 } else { points[k + 1].0 };
            x = x.clamp(lo, hi);
            if g.pinned_ends && (k == 0 || k == last) {
                x = points[k].0;
            }
            points[k] = (x, y);
            changed = true;
        }
        if r.secondary_clicked() && points.len() > 2 && !(g.pinned_ends && (k == 0 || k == last)) {
            remove = Some(k);
        }
        let axis = |v: f32, unit: &str| {
            let v = crate::fmt::num(v as f64, 1);
            if unit.is_empty() {
                v
            } else {
                format!("{v} {}", tr(unit))
            }
        };
        r.on_hover_text(format!(
            "{}, {}",
            axis(points[k].0, g.x_unit),
            axis(points[k].1, g.y_unit)
        ));
    }
    if let Some(k) = remove {
        points.remove(k);
        changed = true;
    }
    if background.double_clicked()
        && points.len() < g.points as usize
        && let Some(p) = background.interact_pointer_pos()
    {
        let new = from_screen(p);
        let k = points.partition_point(|q| q.0 <= new.0);
        points.insert(k, new);
        changed = true;
    }
    if changed {
        set_graph_points(e, at, &points);
    }
}

/// A heading row when `spec` begins a group of its effect's panel.
fn group_heading(ui: &mut egui::Ui, spec: &bv_timeline::ParamSpec) {
    if !spec.group.is_empty() && spec.shown() {
        ui.label(egui::RichText::new(tr(spec.group)).strong());
        ui.end_row();
    }
}

/// The lowest gain a dB field shows before −Inf.
const FLOOR_DB: f32 = -96.0;

/// A number parameter's slider, its value shown as `spec` says: as it is
/// with its unit, a linear gain in dB, a part as a percentage, or a pan.
pub fn param_slider(
    ui: &mut egui::Ui,
    spec: &bv_timeline::ParamSpec,
    v: &mut f32,
) -> egui::Response {
    use bv_timeline::Show;
    let decimals = spec.decimals as usize;
    match spec.show {
        Show::Value => {
            let unit = if spec.unit.is_empty() {
                String::new()
            } else {
                format!(" {}", tr(spec.unit))
            };
            let mut slider = crate::fmt::slider(v, spec.min..=spec.max, decimals, &unit)
                .logarithmic(spec.log && spec.min > 0.0);
            if decimals == 0 {
                slider = slider.step_by(1.0);
            }
            ui.add(slider)
        }
        Show::Db => {
            let floor = spec.min;
            ui.add(
                egui::Slider::new(v, spec.min..=spec.max)
                    .custom_formatter(move |d, _| {
                        if d <= floor as f64 {
                            format!("{} dB", tr("-Inf."))
                        } else {
                            format!("{} dB", crate::fmt::num(d, decimals))
                        }
                    })
                    .custom_parser(move |s| {
                        if s.trim().to_lowercase().contains("inf") {
                            Some(floor as f64)
                        } else {
                            crate::fmt::parse(s)
                        }
                    }),
            )
        }
        Show::Percent => {
            let mut p = *v * 100.0;
            let r = ui.add(crate::fmt::slider(
                &mut p,
                spec.min * 100.0..=spec.max * 100.0,
                decimals,
                " %",
            ));
            *v = p / 100.0;
            r
        }
        Show::GainDb => {
            let top = 20.0 * spec.max.max(1e-6).log10();
            let mut db = if *v > 0.0 {
                (20.0 * v.log10()).max(FLOOR_DB)
            } else {
                FLOOR_DB
            };
            let r = ui.add(
                egui::Slider::new(&mut db, FLOOR_DB..=top)
                    .custom_formatter(move |d, _| {
                        if d <= FLOOR_DB as f64 {
                            format!("{} dB", tr("-Inf."))
                        } else {
                            format!("{} dB", crate::fmt::num(d, decimals))
                        }
                    })
                    .custom_parser(|s| {
                        if s.trim().to_lowercase().contains("inf") {
                            Some(FLOOR_DB as f64)
                        } else {
                            crate::fmt::parse(s)
                        }
                    }),
            );
            if r.changed() {
                *v = if db <= FLOOR_DB {
                    0.0
                } else {
                    10f32.powf(db / 20.0).clamp(spec.min, spec.max)
                };
            }
            r
        }
        Show::Pan => {
            // Shown the other way round: the slider's left is the left.
            let mut d = -*v * 100.0;
            let r = ui.add(
                egui::Slider::new(&mut d, -100.0..=100.0)
                    .custom_formatter(move |d, _| {
                        if d.abs() < 0.05 {
                            tr("Center").to_string()
                        } else if d < 0.0 {
                            format!("{} % L", crate::fmt::num(-d, 1))
                        } else {
                            format!("{} % R", crate::fmt::num(d, 1))
                        }
                    })
                    .custom_parser(crate::fmt::parse),
            );
            if r.changed() {
                *v = (-d / 100.0).clamp(spec.min, spec.max);
            }
            r
        }
    }
}
