//! Audio Properties. The clip owns its adjustments and the FX chain.
//! The track owns ducking because any audible event on it can trigger it.

use super::{CURVES, classic, clip_settings, collapsible_section, curve_name, slider_field};
use crate::app::BananaApp;
use crate::egui::{self, Align, Align2, FontId, Sense, Stroke, Ui, pos2, vec2};
use crate::i18n::tr;
use crate::{theme, widgets};
use bv_core::{FrameRate, Tick};
use bv_timeline::{
    ChannelMap, Clip, Effect, EffectKind, Envelope, EnvelopeKind, EnvelopePoint, KeyCurve,
    MediaInfo, Track,
};

const VOLUME: EnvelopeKind = EnvelopeKind::Volume;
const EQ: EffectKind = EffectKind::ModernEq;
const DENOISE: EffectKind = EffectKind::ExpressAudioRestoration;

pub(super) fn ui(
    ui: &mut Ui,
    app: &mut BananaApp,
    clip: &mut Clip,
    track: &Track,
    media: Option<&MediaInfo>,
    rate: FrameRate,
) {
    let local = (rate.snap_nearest(app.cursor) - clip.start).clamp(Tick::ZERO, clip.duration);
    let id = ui.id().with(("audio-properties", clip.id.0));
    let tray_id = id.with("keyframe-panel");
    let mut tray = ui.data(|d| d.get_temp::<bool>(tray_id)).unwrap_or(false);
    let mut ducking = track.duck_others;
    let height = (ui.available_height() - 38.0).max(45.0);
    egui::ScrollArea::vertical()
        .id_salt(id)
        .max_height(height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            waveform(ui, app, clip, media, id);
            ui.add_enabled_ui(!clip.locked, |ui| {
                adjust(ui, app, clip, &mut ducking, !track.locked, local, rate);
                denoise(ui, clip);
            });
            collapsible_section(ui, tr("Clip Settings"), false, |ui| {
                clip_settings(ui, clip, rate)
            });
            if tray {
                classic::body(ui, |ui| volume_keys(ui, app, clip));
                let opened = tray_id.with("scrolled");
                if !ui.data(|d| d.get_temp::<bool>(opened)).unwrap_or(false) {
                    ui.scroll_to_cursor(Some(Align::BOTTOM));
                    ui.data_mut(|d| d.insert_temp(opened, true));
                }
            } else {
                ui.data_mut(|d| d.remove::<bool>(tray_id.with("scrolled")));
            }
        });
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!clip.locked, egui::Button::new(tr("Reset")))
            .clicked()
        {
            reset_adjustment(clip, &mut ducking, !track.locked);
            reset_fx(clip, DENOISE);
        }
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add(egui::Button::new(tr("Keyframe Panel")).selected(tray))
                .clicked()
            {
                tray = !tray;
            }
        });
    });
    ui.data_mut(|d| d.insert_temp(tray_id, tray));
    if ducking != track.duck_others && !track.locked {
        if track.duck_others.is_none() && ducking.is_some() {
            app.duck_failures
                .retain(|(id, _, _)| !track.clips.iter().any(|c| c.id == *id));
        }
        app.modify_merged(id.with("audio-ducking"), |p| {
            if let Some(track) = p.track_by_id_mut(track.id) {
                track.duck_others = ducking;
            }
        });
    }
}

fn waveform(
    ui: &mut Ui,
    app: &mut BananaApp,
    clip: &Clip,
    media: Option<&MediaInfo>,
    id: egui::Id,
) {
    let zoom_id = id.with("waveform-zoom");
    let mut zoom = ui.data(|d| d.get_temp::<f32>(zoom_id)).unwrap_or(1.0);
    let (r, response) = ui.allocate_exact_size(vec2(ui.available_width(), 92.0), Sense::click());
    let p = ui.painter_at(r);
    p.rect_filled(r, 6.0, theme::clip_audio().gamma_multiply(0.45));
    let mut sound = r.shrink2(vec2(7.0, 4.0));
    sound.set_top(r.top() + 26.0);
    let waveform_painter = p.with_clip_rect(sound);
    let mid = sound.center().y;
    waveform_painter.hline(
        sound.x_range(),
        mid,
        Stroke::new(1.0, theme::clip_audio().gamma_multiply(0.52)),
    );
    let relative = (app.cursor - clip.start).clamp(Tick::ZERO, clip.duration);
    let inside = app.cursor >= clip.start && app.cursor < clip.end();
    let span = clip.duration.as_seconds_f64() / f64::from(zoom);
    let limit = (clip.duration.as_seconds_f64() - span).max(0.0);
    let center = if inside {
        relative.as_seconds_f64()
    } else {
        span * 0.5
    };
    let start = (center - span * 0.5).clamp(0.0, limit);
    let local_at = |fraction: f64| Tick::from_seconds_f64(start + span * fraction);
    if let Some(media) = media
        && let Some(wave) = app.engine.waveform(media, clip.audio_stream)
    {
        let columns = sound.width().clamp(2.0, 1200.0).round() as usize;
        let duration = media.duration.max(Tick::ZERO);
        let peaks: Vec<_> = (0..columns)
            .map(|i| {
                let at = |n| clip.source_time(clip.start + local_at(n as f64 / columns as f64));
                let (a, b) = (at(i), at(i + 1));
                waveform_peak(&wave, a.min(b), a.max(b), duration, clip.looped)
            })
            .collect();
        let scale = waveform_reference(&peaks);
        let half = sound.height() * 0.46;
        let color = theme::clip_audio();
        let mut mesh = egui::Mesh::default();
        for (i, &(lo, hi)) in peaks.iter().enumerate() {
            let x = sound.left() + sound.width() * i as f32 / (columns - 1) as f32;
            mesh.colored_vertex(pos2(x, mid - display_peak(hi, scale) * half), color);
            mesh.colored_vertex(pos2(x, mid - display_peak(lo, scale) * half), color);
            if i > 0 {
                let top = 2 * i as u32;
                mesh.add_triangle(top - 2, top - 1, top);
                mesh.add_triangle(top, top - 1, top + 1);
            }
        }
        waveform_painter.add(egui::Shape::mesh(mesh));
    }
    if inside && span > 0.0 {
        let x = sound.left()
            + sound.width() * ((relative.as_seconds_f64() - start) / span).clamp(0.0, 1.0) as f32;
        p.vline(x, sound.y_range(), Stroke::new(1.0, theme::cursor()));
    }
    let slider = egui::Rect::from_min_size(pos2(r.right() - 85.0, r.top() + 3.0), vec2(77.0, 18.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(slider), |ui| {
        ui.spacing_mut().slider_width = slider.width();
        ui.add(
            egui::Slider::new(&mut zoom, 1.0..=12.0)
                .logarithmic(true)
                .show_value(false),
        )
        .on_hover_text(tr("Waveform Zoom"));
    });
    ui.data_mut(|d| d.insert_temp(zoom_id, zoom));
    if let Some(media) = media {
        widgets::fit_text(
            &p,
            r.left_top() + vec2(8.0, 5.0),
            Align2::LEFT_TOP,
            crate::i18n::media_name(media),
            FontId::proportional(12.0),
            theme::text(),
            r.width() - 100.0,
        );
    }
    if response.clicked()
        && let Some(pos) = response
            .interact_pointer_pos()
            .filter(|p| sound.contains(*p))
    {
        let fraction = f64::from((pos.x - sound.left()) / sound.width().max(1.0));
        app.seek(clip.start + local_at(fraction));
    }
}

/// Combine the stereo peaks for a column, preserving silence outside the
/// source and splitting ranges that cross the end of a loop.
fn waveform_peak(
    wave: &bv_engine::Waveform,
    a: Tick,
    b: Tick,
    duration: Tick,
    looped: bool,
) -> (f32, f32) {
    let range = |from, to| {
        (0..wave.peaks.len().min(2)).fold((0.0f32, 0.0f32), |(lo, hi), ch| {
            let (min, max) = wave.range(ch, from, to);
            (lo.min(min), hi.max(max))
        })
    };
    if duration <= Tick::ZERO {
        return (0.0, 0.0);
    }
    if !looped {
        return if b <= Tick::ZERO || a >= duration {
            (0.0, 0.0)
        } else {
            range(a.max(Tick::ZERO), b.min(duration))
        };
    }
    if b - a >= duration {
        return range(Tick::ZERO, duration);
    }
    let start = Tick(a.0.rem_euclid(duration.0));
    let end = start + (b - a);
    if end <= duration {
        range(start, end)
    } else {
        let (lo, hi) = range(start, duration);
        let (wrap_lo, wrap_hi) = range(Tick::ZERO, end - duration);
        (lo.min(wrap_lo), hi.max(wrap_hi))
    }
}

/// Scale the drawing to the visible audio with a floor so low-level noise
/// does not fill the display. This is independent of clip gain.
fn waveform_reference(peaks: &[(f32, f32)]) -> f32 {
    peaks.iter().fold(0.01f32, |reference, (lo, hi)| {
        reference.max(lo.abs()).max(hi.abs())
    })
}

fn display_peak(amplitude: f32, reference: f32) -> f32 {
    ((amplitude.abs() / reference).clamp(0.0, 1.0))
        .sqrt()
        .copysign(amplitude)
}

fn adjust(
    ui: &mut Ui,
    app: &mut BananaApp,
    clip: &mut Clip,
    duck: &mut Option<f32>,
    can_duck: bool,
    local: Tick,
    rate: FrameRate,
) {
    let curve = app.settings.prefs.new_envelope_curve(VOLUME);
    let mut reset = false;
    let modified = clip.level != 1.0
        || clip.envelope(VOLUME).is_some()
        || clip.audio_balance != 0.0
        || clip.channels != ChannelMap::Both
        || clip.fade_in != Tick::ZERO
        || clip.fade_out != Tick::ZERO
        || clip.fade_in_curve != Default::default()
        || clip.fade_out_curve != Default::default()
        || clip.pitch != 0.0
        || clip.auto_normalize
        || duck.is_some()
        || effect_index(clip, EQ).is_some_and(|i| clip.effects[i].params != Effect::new(EQ).params);
    let open = classic::header(ui, tr("Adjustment"), None, true, |ui| {
        reset = modified && reset_button(ui);
    });
    if reset {
        reset_adjustment(clip, duck, can_duck);
    }
    if !open {
        return;
    }
    classic::body(ui, |ui| {
        volume(ui, clip, local, curve);
        ui.push_id("audio-channels", |ui| {
            if row(ui, tr("Audio Channels"), clip.channels != ChannelMap::Both) {
                clip.channels = ChannelMap::Both;
            }
            egui::ComboBox::from_id_salt("mode")
                .width(ui.available_width())
                .selected_text(channel_name(clip.channels))
                .show_ui(ui, |ui| {
                    for mode in [
                        ChannelMap::Both,
                        ChannelMap::LeftOnly,
                        ChannelMap::RightOnly,
                        ChannelMap::Combine,
                        ChannelMap::Swap,
                    ] {
                        ui.selectable_value(&mut clip.channels, mode, channel_name(mode));
                    }
                });
        });
        let mut balance = clip.audio_balance * 100.0;
        control(
            ui,
            tr("Sound Balance"),
            &mut balance,
            0.0,
            -100.0..=100.0,
            0,
            " %",
        );
        clip.audio_balance = balance / 100.0;
        let max = clip.duration.as_seconds_f64() as f32;
        for (label, fade, shape) in [
            (tr("Fade in"), &mut clip.fade_in, &mut clip.fade_in_curve),
            (tr("Fade out"), &mut clip.fade_out, &mut clip.fade_out_curve),
        ] {
            ui.push_id(label, |ui| {
                if row(
                    ui,
                    label,
                    *fade != Tick::ZERO || *shape != Default::default(),
                ) {
                    *fade = Tick::ZERO;
                    *shape = Default::default();
                }
                let mut seconds = fade.as_seconds_f64() as f32;
                if slider_field(ui, &mut seconds, 0.0..=max, 2, " s") {
                    *fade = rate
                        .snap_nearest(Tick::from_seconds_f64(f64::from(seconds)))
                        .min(clip.duration);
                }
                egui::ComboBox::from_id_salt("curve")
                    .selected_text(curve_name(*shape))
                    .show_ui(ui, |ui| {
                        for (mode, name) in CURVES {
                            ui.selectable_value(shape, mode, tr(name));
                        }
                    });
            });
        }
        control(
            ui,
            tr("Pitch"),
            &mut clip.pitch,
            0.0,
            -48.0..=48.0,
            2,
            " st",
        );
        ui.add_enabled_ui(can_duck, |ui| ducking(ui, app, duck));
        equalizer(ui, app, clip);
        crate::widgets::checkbox(ui, &mut clip.auto_normalize, tr("Auto Normalize"));
    });
}

fn reset_button(ui: &mut Ui) -> bool {
    widgets::icon_button(ui, crate::icons::Icon::Undo, tr("Reset")).clicked()
}

fn row(ui: &mut Ui, label: &str, changed: bool) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            changed && reset_button(ui)
        })
        .inner
    })
    .inner
}

fn control(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    default: f32,
    range: std::ops::RangeInclusive<f32>,
    decimals: usize,
    suffix: &str,
) -> bool {
    ui.push_id(label, |ui| {
        let reset = row(ui, label, (*value - default).abs() > 0.0001);
        if reset {
            *value = default;
        }
        slider_field(ui, value, range, decimals, suffix) || reset
    })
    .inner
}

fn channel_name(mode: ChannelMap) -> &'static str {
    match mode {
        ChannelMap::Both => tr("Stereo"),
        ChannelMap::LeftOnly => tr("Copy left to right"),
        ChannelMap::RightOnly => tr("Copy right to left"),
        ChannelMap::Combine => tr("Combine"),
        ChannelMap::Swap => tr("Swap"),
    }
}

fn ducking(ui: &mut Ui, app: &BananaApp, value: &mut Option<f32>) {
    let mut enabled = value.is_some();
    let mut pct = value.unwrap_or(0.5) * 100.0;
    ui.push_id("audio-ducking", |ui| {
        ui.horizontal(|ui| {
            crate::widgets::checkbox(ui, &mut enabled, tr("Audio Ducking")).on_hover_text(tr(
                "Uses this audio track to lower all other audio tracks while it sounds.",
            ));
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if value.is_some() && reset_button(ui) {
                    enabled = false;
                    pct = 50.0;
                }
            });
        });
        ui.add_enabled_ui(enabled, |ui| {
            slider_field(ui, &mut pct, 0.0..=100.0, 0, " %")
        });
        if enabled && app.engine.ducking_progress().is_some_and(|p| !p.finished) {
            ui.label(tr("Reading the sound…"));
        }
    });
    *value = enabled.then_some(pct / 100.0);
}

fn volume(ui: &mut Ui, clip: &mut Clip, local: Tick, curve: KeyCurve) {
    ui.push_id("volume", |ui| {
        let envelope = clip.envelope(VOLUME);
        let keyed = envelope.is_some_and(|e| e.points.iter().any(|point| point.at == local));
        let mut db = super::bv_mixer_gain_db(clip.audio_gain_at(clip.start + local));
        let (key, reset) = ui
            .horizontal(|ui| {
                ui.label(tr("Volume"));
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    let key = classic::diamond(ui, keyed, tr("Add Volume Keyframe"));
                    let reset = (envelope.is_some() || clip.level != 1.0) && reset_button(ui);
                    (key, reset)
                })
                .inner
            })
            .inner;
        if reset {
            clear_volume(clip);
            db = 0.0;
        }
        let edited = slider_field(ui, &mut db, -60.0..=12.0, 2, " dB");
        if key || edited {
            let gain = if db <= -59.9 {
                0.0
            } else {
                10f32.powf(db / 20.0)
            };
            if key || clip.envelope(VOLUME).is_some() {
                set_volume_key(clip, local, gain, curve);
            } else {
                clip.level = gain;
            }
        }
    });
}

fn set_volume_key(clip: &mut Clip, local: Tick, gain: f32, curve: KeyCurve) {
    if clip.envelope(VOLUME).is_none() {
        let mut envelope = Envelope::with_curve(VOLUME, curve);
        envelope.points[0].value = clip.level;
        clip.envelopes.push(envelope);
    }
    if let Some(envelope) = clip.envelopes.iter_mut().find(|e| e.kind == VOLUME) {
        match envelope.points.binary_search_by_key(&local, |p| p.at) {
            Ok(i) => envelope.points[i].value = gain,
            Err(i) => envelope.points.insert(
                i,
                EnvelopePoint {
                    at: local,
                    value: gain,
                    curve,
                },
            ),
        }
    }
}

fn clear_volume(clip: &mut Clip) {
    clip.level = 1.0;
    clip.envelopes.retain(|e| e.kind != VOLUME);
}

fn volume_keys(ui: &mut Ui, app: &mut BananaApp, clip: &mut Clip) {
    ui.strong(tr("Volume Keyframes"));
    let Some(envelope) = clip.envelopes.iter_mut().find(|e| e.kind == VOLUME) else {
        ui.label(tr("No keyframes"));
        return;
    };
    let points: Vec<_> = envelope.points.iter().map(|p| p.at).collect();
    if let Some(i) = classic::keyframe_rows(ui, app, clip.start, &points, !clip.locked) {
        let removed = envelope.points.remove(i);
        if envelope.points.is_empty() {
            clip.level = removed.value;
            clip.envelopes.retain(|e| e.kind != VOLUME);
        }
    }
}

fn effect_index(clip: &Clip, kind: EffectKind) -> Option<usize> {
    clip.effects.iter().position(|effect| effect.kind == kind)
}

fn effect(clip: &mut Clip, kind: EffectKind) -> usize {
    effect_index(clip, kind).unwrap_or_else(|| {
        let i = clip.effects.len();
        clip.effects.push(Effect::new(kind));
        i
    })
}

fn reset_fx(clip: &mut Clip, kind: EffectKind) {
    if let Some(i) = effect_index(clip, kind) {
        clip.effects[i].params = Effect::new(kind).params;
        if kind == DENOISE {
            clip.effects[i].enabled = false;
        }
    }
}

fn reset_adjustment(clip: &mut Clip, duck: &mut Option<f32>, can_duck: bool) {
    clear_volume(clip);
    clip.audio_balance = 0.0;
    clip.channels = ChannelMap::Both;
    clip.fade_in = Tick::ZERO;
    clip.fade_out = Tick::ZERO;
    clip.fade_in_curve = Default::default();
    clip.fade_out_curve = Default::default();
    clip.pitch = 0.0;
    clip.auto_normalize = false;
    if can_duck {
        *duck = None;
    }
    reset_fx(clip, EQ);
}

fn equalizer(ui: &mut Ui, app: &mut BananaApp, clip: &mut Clip) {
    let flat = Effect::new(EQ);
    let presets: Vec<_> = crate::presets::builtin(EQ)
        .into_iter()
        .map(|(name, values)| {
            let mut selected = flat.clone();
            for (i, value) in values {
                if let Some(p) = selected.params.get_mut(i) {
                    *p = value;
                }
            }
            (tr(name), selected.params)
        })
        .collect();
    let current = effect_index(clip, EQ)
        .and_then(|i| clip.effects.get(i))
        .filter(|e| e.enabled);
    let name = current.map_or(tr("Default"), |e| {
        if e.params == flat.params {
            tr("Default")
        } else {
            presets
                .iter()
                .find(|(_, parameters)| *parameters == e.params)
                .map_or(tr("Custom"), |(name, _)| *name)
        }
    });
    if row(ui, tr("Equalizer"), name != tr("Default")) {
        reset_fx(clip, EQ);
    }
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("audio-eq-preset")
            .width((ui.available_width() - 80.0).max(60.0))
            .selected_text(name)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(name == tr("Default"), tr("Default"))
                    .clicked()
                {
                    reset_fx(clip, EQ);
                }
                for (label, parameters) in presets {
                    if ui.selectable_label(name == label, label).clicked() {
                        let i = effect(clip, EQ);
                        clip.effects[i].params = parameters;
                        clip.effects[i].enabled = true;
                    }
                }
            });
        if ui.button(tr("Settings")).clicked() {
            let i = effect(clip, EQ);
            app.clip_windows.fx_node = Some((crate::fx_window::FxTarget::Clip(clip.id), i));
        }
    });
}

fn denoise(ui: &mut Ui, clip: &mut Clip) {
    collapsible_section(ui, tr("Denoise"), true, |ui| {
        let mut enabled = effect_index(clip, DENOISE).is_some_and(|i| clip.effects[i].enabled);
        if widgets::checkbox(ui, &mut enabled, tr("Noise Reduction")).changed() {
            let i = effect(clip, DENOISE);
            clip.effects[i].enabled = enabled;
        }
        if let Some(i) = effect_index(clip, DENOISE) {
            let effect = &mut clip.effects[i];
            ui.add_enabled_ui(effect.enabled, |ui| {
                let param = bv_timeline::audio_fx::express_audio_restoration::REDUCTION;
                let mut db = effect.param(param);
                if control(
                    ui,
                    tr("Reduce noise by"),
                    &mut db,
                    12.0,
                    0.0..=100.0,
                    1,
                    " dB",
                ) && let Some(value) = effect.params.get_mut(param)
                {
                    *value = db;
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    #[test]
    fn a_quiet_waveform_fills_the_graph_without_moving_its_center() {
        let peaks = [(0.0, 0.0), (-0.022, 0.022), (-0.0022, 0.0022)];
        let reference = waveform_reference(&peaks);
        assert_eq!(display_peak(0.0, reference), 0.0);
        assert_eq!(display_peak(-0.022, reference), -1.0);
        assert_eq!(display_peak(0.022, reference), 1.0);
        assert!(display_peak(0.0022, reference) > 0.25);
        assert!(display_peak(0.00001, waveform_reference(&[(0.0, 0.00001)])) < 0.04);
    }

    #[test]
    fn audio_properties_saves_a_clip_volume_key_from_the_header() {
        let mut app = BananaApp::new(egui::Context::default());
        let id = app.modify(|p| {
            let track = p.add_track(bv_timeline::TrackKind::Audio);
            let media = p.add_media(MediaInfo::generated(
                bv_timeline::Generator::TestTone {
                    frequency_hz: 440.0,
                    level_db: -12.0,
                },
                &p.settings,
            ));
            p.insert_media(media, Tick::ZERO, None, Some(track))
                .unwrap()[0]
        });
        app.select_clip(id, false);
        app.seek(Tick::from_seconds(1));
        let mut harness = Harness::builder()
            .with_size(vec2(410.0, 1160.0))
            .build_ui_state(|ui, app| super::super::ui(ui, app), app);
        harness.get_by_label("Adjustment");
        harness.get_by_label("Add Volume Keyframe").click();
        harness.run();
        let clip = harness.state().project().clip(id).unwrap();
        let volume = clip.envelope(VOLUME).unwrap();
        assert_eq!(volume.points.len(), 2);
        assert_eq!(
            volume.points[1].at,
            harness
                .state()
                .project()
                .settings
                .frame_rate
                .snap_nearest(Tick::from_seconds(1)),
        );
        harness.get_by_label("Keyframe Panel").click();
        harness.run();
        harness.get_by_label("Volume Keyframes");
    }
}
