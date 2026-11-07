//! The everyday MP4 export dialog. The renderer chooses the available backend.

use bv_core::{FrameRate, Tick, TimeRange};
use bv_engine::{
    AudioParams, Encoder, FileFormat, RateMode, RenderOptions, RenderSettings, VideoParams,
};
use serde::{Deserialize, Serialize};

use super::{Action, RenderDialog, templates};
use crate::app::BananaApp;
use crate::egui::{self, Align, Layout, RichText, Ui, vec2};
use crate::i18n::tr;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct SimpleSettings {
    #[serde(default)]
    cbr: bool,
    hevc: bool,
    short_edge: u32,
    custom_size: (u32, u32),
    rate: Option<FrameRate>,
    quality: usize,
    video_kbps: u32,
    sample_rate: u32,
    channels: u16,
    audio_kbps: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct NamedPreset {
    pub name: String,
    settings: SimpleSettings,
}

#[derive(Debug)]
pub(super) struct PresetEdit {
    name: String,
    rename: bool,
}

impl Default for SimpleSettings {
    fn default() -> Self {
        Self {
            cbr: false,
            hevc: false,
            short_edge: 0,
            custom_size: (2560, 1440),
            rate: None,
            quality: 1,
            video_kbps: 24_000,
            sample_rate: 48_000,
            channels: 2,
            audio_kbps: 192,
        }
    }
}

impl SimpleSettings {
    fn format(self) -> FileFormat {
        if self.hevc {
            FileFormat::HevcMp4
        } else {
            FileFormat::AvcMp4
        }
    }

    fn size(self, (w, h): (u32, u32)) -> (u32, u32) {
        let (w, h) = (w.max(2), h.max(2));
        let short = self.short_edge;
        let even = |v: u32| (v & !1).max(2);
        if short == 0 {
            return (even(w), even(h));
        }
        if short == 1 {
            return (
                even(self.custom_size.0.clamp(2, 8192)),
                even(self.custom_size.1.clamp(2, 8192)),
            );
        }
        let scale = f64::from(short) / f64::from(w.min(h));
        (
            even((f64::from(w) * scale).round() as u32),
            even((f64::from(h) * scale).round() as u32),
        )
    }

    fn bitrate(self, (w, h): (u32, u32), rate: FrameRate) -> u32 {
        if self.quality == 3 {
            return self.video_kbps.clamp(750, 150_000) * 1000;
        }
        let quality = [0.65, 1.0, 1.4][self.quality.min(2)];
        let codec = if self.hevc { 0.85 } else { 1.0 };
        (24_000_000.0
            * (f64::from(w) * f64::from(h) / (2560.0 * 1440.0)).powf(0.75)
            * (rate.as_f64() / 60.0).sqrt()
            * quality
            * codec)
            .clamp(750_000.0, 150_000_000.0)
            .round() as u32
    }
}

fn choose<T: Copy + PartialEq>(
    ui: &mut Ui,
    label: &str,
    value: &mut T,
    choices: impl IntoIterator<Item = (T, String)>,
    current: impl Fn(T) -> String,
) {
    ui.label(label);
    egui::ComboBox::from_id_salt(label)
        .width(230.0)
        .selected_text(current(*value))
        .show_ui(ui, |ui| {
            for (choice, text) in choices {
                ui.selectable_value(value, choice, text);
            }
        });
    ui.end_row();
}

impl RenderDialog {
    pub(super) fn show_simple(&mut self, ui: &mut Ui, app: &mut BananaApp) -> Action {
        ui.set_min_width(545.0);
        let before = self.simple;
        let mut loaded = false;
        ui.spacing_mut().item_spacing.y = 8.0;
        egui::Grid::new("simple-export-destination")
            .num_columns(2)
            .spacing([18.0, 8.0])
            .show(ui, |ui| {
                ui.label(tr("Name:"));
                ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(230.0));
                ui.end_row();
                ui.label(tr("Folder:"));
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.folder).desired_width(230.0));
                    if ui.button(tr("Browse…")).clicked()
                        && let Some(folder) = rfd::FileDialog::new()
                            .set_directory(&self.folder)
                            .pick_folder()
                    {
                        self.folder = folder.display().to_string();
                    }
                });
                ui.end_row();
            });
        ui.horizontal(|ui| {
            ui.label(tr("Preset"));
            egui::ComboBox::from_id_salt("named-export-preset")
                .width(200.0)
                .selected_text(self.simple_preset.as_deref().unwrap_or(tr("Custom")))
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.simple_preset.is_none(), tr("Custom"))
                        .clicked()
                    {
                        self.simple_preset = None;
                    }
                    for preset in &self.simple_presets {
                        if ui
                            .selectable_label(
                                self.simple_preset.as_ref() == Some(&preset.name),
                                &preset.name,
                            )
                            .clicked()
                        {
                            self.simple = preset.settings;
                            self.simple_preset = Some(preset.name.clone());
                            loaded = true;
                        }
                    }
                });
            if ui.button(tr("Settings")).clicked() {
                self.simple_details = true;
            }
            if crate::widgets::icon_button(ui, crate::icons::Icon::Save, tr("Save Preset…"))
                .clicked()
            {
                self.preset_error = None;
                self.preset_edit = Some(PresetEdit {
                    name: String::new(),
                    rename: false,
                });
            }
            if crate::widgets::icon_button_ex(
                ui,
                crate::icons::Icon::Pencil,
                tr("Rename Preset"),
                false,
                self.simple_preset.is_some(),
            )
            .clicked()
            {
                self.preset_error = None;
                self.preset_edit = Some(PresetEdit {
                    name: self.simple_preset.clone().unwrap_or_default(),
                    rename: true,
                });
            }
            if crate::widgets::icon_button_ex(
                ui,
                crate::icons::Icon::Delete,
                tr("Delete Preset"),
                false,
                self.simple_preset.is_some(),
            )
            .clicked()
            {
                let mut presets = self.simple_presets.clone();
                presets.retain(|p| Some(&p.name) != self.simple_preset.as_ref());
                if self.store_presets(app, presets) {
                    self.simple_preset = None;
                }
            }
        });
        if let Some(error) = &self.preset_error {
            ui.colored_label(crate::theme::error(), error);
        }
        ui.separator();
        ui.label(RichText::new(tr("Video")).strong());
        let project = app.project();
        let simple = &mut self.simple;
        egui::Grid::new("simple-export-video")
            .num_columns(2)
            .spacing([18.0, 8.0])
            .show(ui, |ui| {
                let size_text = |edge| {
                    let (w, h) = SimpleSettings {
                        short_edge: edge,
                        ..*simple
                    }
                    .size(project.settings.output_size());
                    if edge == 0 {
                        format!("{} ({w} × {h})", tr("Project"))
                    } else if edge == 1 {
                        tr("Custom").to_string()
                    } else {
                        format!("{w} × {h}")
                    }
                };
                let sizes: Vec<_> = [0, 720, 1080, 1440, 2160, 1]
                    .into_iter()
                    .map(|edge| (edge, size_text(edge)))
                    .collect();
                let current_size = size_text(simple.short_edge);
                choose(ui, tr("Resolution"), &mut simple.short_edge, sizes, |_| {
                    current_size.clone()
                });
                if simple.short_edge == 1 {
                    ui.label("");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut simple.custom_size.0).range(2..=8192));
                        ui.label("×");
                        ui.add(egui::DragValue::new(&mut simple.custom_size.1).range(2..=8192));
                    });
                    ui.end_row();
                }
                let rates = [
                    None,
                    Some(FrameRate::FPS_23_976),
                    Some(FrameRate::FPS_24),
                    Some(FrameRate::FPS_25),
                    Some(FrameRate::FPS_29_97),
                    Some(FrameRate::FPS_30),
                    Some(FrameRate::FPS_50),
                    Some(FrameRate::FPS_59_94),
                    Some(FrameRate::FPS_60),
                ];
                let rate_text = |rate: Option<FrameRate>| {
                    let text = format!(
                        "{} fps",
                        templates::rate_text(rate.unwrap_or(project.settings.frame_rate))
                    );
                    if rate.is_none() {
                        format!("{} ({text})", tr("Project"))
                    } else {
                        text
                    }
                };
                choose(
                    ui,
                    tr("Frame Rate"),
                    &mut simple.rate,
                    rates.into_iter().map(|rate| (rate, rate_text(rate))),
                    rate_text,
                );
                let codec = |hevc| {
                    if hevc {
                        "HEVC (H.265)".into()
                    } else {
                        "AVC (H.264)".into()
                    }
                };
                choose(
                    ui,
                    tr("Codec"),
                    &mut simple.hevc,
                    [false, true].into_iter().map(|v| (v, codec(v))),
                    codec,
                );
                ui.label(tr("Quality"));
                ui.horizontal(|ui| {
                    for (i, text) in [tr("Smaller file"), tr("Recommended"), tr("Higher quality")]
                        .into_iter()
                        .enumerate()
                    {
                        ui.radio_value(&mut simple.quality, i, text);
                    }
                });
                ui.end_row();
                choose(
                    ui,
                    tr("Bitrate Mode"),
                    &mut simple.cbr,
                    [(false, "VBR".into()), (true, "CBR".into())],
                    |v| if v { "CBR".into() } else { "VBR".into() },
                );
                let bitrate = simple.bitrate(
                    simple.size(project.settings.output_size()),
                    simple.rate.unwrap_or(project.settings.frame_rate),
                );
                ui.label(tr("Bit rate:"));
                let mut kbps = bitrate / 1000;
                if ui
                    .add(
                        egui::DragValue::new(&mut kbps)
                            .range(750..=150_000)
                            .suffix(" kbps"),
                    )
                    .changed()
                {
                    simple.video_kbps = kbps;
                    simple.quality = 3;
                }
                ui.end_row();
                ui.label(tr("Format"));
                ui.label("MP4 · SDR Rec.709");
                ui.end_row();
            });
        ui.separator();
        ui.collapsing(tr("Audio Settings"), |ui| {
            egui::Grid::new("simple-export-audio")
                .num_columns(2)
                .spacing([18.0, 8.0])
                .show(ui, |ui| {
                    ui.label(tr("Codec"));
                    ui.label("AAC");
                    ui.end_row();
                    let channels = |v| {
                        if v == 1 {
                            tr("Mono").into()
                        } else {
                            tr("Stereo").into()
                        }
                    };
                    choose(
                        ui,
                        tr("Channels"),
                        &mut simple.channels,
                        [1, 2].into_iter().map(|v| (v, channels(v))),
                        channels,
                    );
                    let hz = |v| format!("{v} Hz");
                    choose(
                        ui,
                        tr("Sample rate"),
                        &mut simple.sample_rate,
                        [44_100, 48_000].into_iter().map(|v| (v, hz(v))),
                        hz,
                    );
                    let kbps = |v| format!("{v} kbps");
                    choose(
                        ui,
                        tr("Bit rate:"),
                        &mut simple.audio_kbps,
                        [128, 192, 256, 320].into_iter().map(|v| (v, kbps(v))),
                        kbps,
                    );
                });
        });
        if project.loop_region.is_some() {
            ui.checkbox(&mut self.loop_only, tr("Render loop region only"));
        }
        ui.separator();
        if self.simple != before && !loaded {
            self.simple_preset = None;
        }
        let ready = self.simple_render_settings(app).is_some();
        let range = app
            .project()
            .loop_region
            .filter(|_| self.loop_only)
            .unwrap_or_else(|| TimeRange::new(Tick::ZERO, app.project().duration()));
        let seconds = range.duration.as_seconds_f64().max(0.0);
        let bitrate = self.simple.bitrate(
            self.simple.size(app.project().settings.output_size()),
            self.simple
                .rate
                .unwrap_or(app.project().settings.frame_rate),
        );
        let mb = estimated_mb(seconds, bitrate, self.simple.audio_kbps);
        let duration = duration_text(seconds);
        ui.horizontal(|ui| {
            ui.label(trf!(
                "Duration: {} · Size: {} MB (estimated)",
                duration,
                crate::fmt::num(mb, 2)
            ));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let button = egui::Button::new(RichText::new(tr("Export")).strong())
                    .fill(crate::theme::ACTION_BLUE)
                    .min_size(vec2(96.0, 30.0));
                if ui.add_enabled(ready, button).clicked() {
                    Action::Render
                } else if !self.simple_details
                    && self.preset_edit.is_none()
                    && ui.input(|i| i.key_pressed(egui::Key::Escape))
                {
                    Action::Close
                } else {
                    Action::None
                }
            })
            .inner
        })
        .inner
    }

    fn store_presets(&mut self, app: &mut BananaApp, presets: Vec<NamedPreset>) -> bool {
        let previous = std::mem::replace(&mut app.settings.render.simple_presets, presets.clone());
        match app.settings.save() {
            Ok(()) => {
                self.simple_presets = presets;
                self.preset_error = None;
                true
            }
            Err(error) => {
                app.settings.render.simple_presets = previous;
                self.preset_error = Some(trf!("Could not save presets: {}", error));
                false
            }
        }
    }

    pub(super) fn simple_windows(&mut self, ctx: &egui::Context, app: &mut BananaApp) {
        if self.simple_details {
            let before = self.simple;
            crate::floating::Window::new(tr("Export Settings"))
                .id(egui::Id::new("simple-export-settings"))
                .open(&mut self.simple_details)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(tr("Bit rate:"));
                    let mut kbps = self.simple.bitrate(
                        self.simple.size(app.project().settings.output_size()),
                        self.simple
                            .rate
                            .unwrap_or(app.project().settings.frame_rate),
                    ) / 1000;
                    if ui
                        .add(
                            egui::DragValue::new(&mut kbps)
                                .range(750..=150_000)
                                .suffix(" kbps"),
                        )
                        .changed()
                    {
                        self.simple.video_kbps = kbps;
                        self.simple.quality = 3;
                    }
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.simple.cbr, false, "VBR");
                        ui.radio_value(&mut self.simple.cbr, true, "CBR");
                    });
                    ui.separator();
                    ui.strong(tr("Audio Settings"));
                    egui::Grid::new("preset-audio-settings").show(ui, |ui| {
                        choose(
                            ui,
                            tr("Channels"),
                            &mut self.simple.channels,
                            [(1, tr("Mono").into()), (2, tr("Stereo").into())],
                            |v| {
                                if v == 1 {
                                    tr("Mono").into()
                                } else {
                                    tr("Stereo").into()
                                }
                            },
                        );
                        choose(
                            ui,
                            tr("Sample rate"),
                            &mut self.simple.sample_rate,
                            [44_100, 48_000].into_iter().map(|v| (v, format!("{v} Hz"))),
                            |v| format!("{v} Hz"),
                        );
                        choose(
                            ui,
                            tr("Bit rate:"),
                            &mut self.simple.audio_kbps,
                            [128, 192, 256, 320]
                                .into_iter()
                                .map(|v| (v, format!("{v} kbps"))),
                            |v| format!("{v} kbps"),
                        );
                    });
                });
            if before != self.simple {
                self.simple_preset = None;
            }
        }
        let Some(edit) = &mut self.preset_edit else {
            return;
        };
        let mut open = true;
        let mut save = false;
        crate::floating::Window::new(if edit.rename {
            tr("Rename Preset")
        } else {
            tr("Save Preset…")
        })
        .id(egui::Id::new("simple-preset-name"))
        .open(&mut open)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(tr("Preset name"));
            ui.add(egui::TextEdit::singleline(&mut edit.name).char_limit(80));
            if let Some(error) = &self.preset_error {
                ui.colored_label(crate::theme::error(), error);
            }
            save = ui.button(tr("Save")).clicked();
        });
        if !open {
            self.preset_edit = None;
            self.preset_error = None;
            return;
        }
        if save {
            let edit = self
                .preset_edit
                .as_ref()
                .map(|e| (e.name.clone(), e.rename));
            if let Some((name, rename)) = edit {
                let mut presets = self.simple_presets.clone();
                match save_preset(
                    &mut presets,
                    &name,
                    rename.then_some(self.simple_preset.as_deref()).flatten(),
                    self.simple,
                ) {
                    Ok(name) => {
                        if self.store_presets(app, presets) {
                            self.simple_preset = Some(name);
                            self.preset_edit = None;
                        }
                    }
                    Err(error) => self.preset_error = Some(tr(error).into()),
                }
            }
        }
    }

    pub(super) fn simple_render_settings(&self, app: &BananaApp) -> Option<RenderSettings> {
        let project = app.project();
        let simple = self.simple;
        let format = simple.format();
        let range = project
            .loop_region
            .filter(|_| self.loop_only)
            .unwrap_or_else(|| TimeRange::new(Tick::ZERO, project.duration()));
        if range.duration <= Tick::ZERO || self.folder.trim().is_empty() {
            return None;
        }
        let encoders = format.video_encoders();
        let encoder = encoders
            .iter()
            .copied()
            .find(|e| *e == Encoder::Nvidia)
            .or_else(|| encoders.iter().copied().find(|e| *e != Encoder::Software))
            .or_else(|| encoders.first().copied())?;
        let size = simple.size(project.settings.output_size());
        let rate = simple.rate.unwrap_or(project.settings.frame_rate);
        let bitrate = simple.bitrate(size, rate);
        let name = self.name.trim();
        let name = if name.is_empty() {
            tr("Untitled")
        } else {
            name
        };
        if std::path::Path::new(name).file_name() != Some(std::ffi::OsStr::new(name)) {
            return None;
        }
        let filename = if name.to_ascii_lowercase().ends_with(".mp4") {
            name.to_string()
        } else {
            format!("{name}.mp4")
        };
        let path = std::path::Path::new(self.folder.trim()).join(filename);
        Some(RenderSettings {
            format,
            path,
            range,
            video: Some(VideoParams {
                width: size.0,
                height: size.1,
                frame_rate: rate,
                encoder,
                bitrate,
                rate_mode: if simple.cbr {
                    RateMode::Cbr
                } else {
                    RateMode::Vbr
                },
                max_bitrate: if simple.cbr {
                    bitrate
                } else {
                    bitrate.saturating_mul(3) / 2
                },
                ..VideoParams::default()
            }),
            audio: Some(AudioParams {
                channels: simple.channels,
                sample_rate: simple.sample_rate,
                bitrate: simple.audio_kbps.saturating_mul(1000),
                ..AudioParams::default()
            }),
            options: RenderOptions {
                swap_files: true,
                use_rotation: true,
                native_mp4: true,
                ..RenderOptions::default()
            },
            project_file: app.project_path.clone(),
        })
    }
}

fn estimated_mb(seconds: f64, video_bps: u32, audio_kbps: u32) -> f64 {
    seconds * (f64::from(video_bps) + f64::from(audio_kbps) * 1000.0) / 8_000_000.0
}

fn duration_text(seconds: f64) -> String {
    let total = seconds.floor().max(0.0) as u64;
    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        total / 60 % 60,
        total % 60
    )
}

fn save_preset(
    presets: &mut Vec<NamedPreset>,
    name: &str,
    rename: Option<&str>,
    settings: SimpleSettings,
) -> Result<String, &'static str> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err(tr("Enter a preset name of 1–80 characters."));
    }
    if name.eq_ignore_ascii_case("Custom") {
        return Err(tr("Choose a preset name other than Custom."));
    }
    if presets
        .iter()
        .any(|p| p.name.eq_ignore_ascii_case(name) && Some(p.name.as_str()) != rename)
    {
        return Err(tr("A preset with that name already exists."));
    }
    if let Some(old) = rename {
        if let Some(preset) = presets.iter_mut().find(|p| p.name == old) {
            preset.name = name.into();
        } else {
            return Err(tr("Select a preset to rename."));
        }
    } else {
        if presets.len() >= 64 {
            return Err(tr("You can save up to 64 export presets."));
        }
        presets.push(NamedPreset {
            name: name.into(),
            settings,
        });
    }
    Ok(name.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approved_export_layout_review() {
        let mut app = BananaApp::new(egui::Context::default());
        app.load_demo();
        let mut dialog = RenderDialog::new(&app);
        dialog.simple_presets.push(NamedPreset {
            name: "YouTube 1440p60".into(),
            settings: dialog.simple,
        });
        dialog.simple_preset = Some("YouTube 1440p60".into());
        crate::patch_review::capture("export", vec2(650.0, 680.0), |ui| {
            ui.heading(tr("Export"));
            let action = dialog.show_simple(ui, &mut app);
            assert!(matches!(action, Action::None));
        });
    }

    #[test]
    fn preset_names_are_unique_and_rename_preserves_export_settings() {
        let settings = SimpleSettings {
            cbr: true,
            short_edge: 1440,
            video_kbps: 31_000,
            quality: 3,
            ..SimpleSettings::default()
        };
        let mut presets = Vec::new();
        assert_eq!(
            save_preset(&mut presets, " YouTube 1440p60 ", None, settings).unwrap(),
            "YouTube 1440p60"
        );
        assert!(save_preset(&mut presets, "youtube 1440P60", None, settings).is_err());
        assert!(save_preset(&mut presets, "Custom", None, settings).is_err());
        assert!(save_preset(&mut presets, "", None, settings).is_err());
        save_preset(
            &mut presets,
            "My NVIDIA preset",
            Some("YouTube 1440p60"),
            SimpleSettings::default(),
        )
        .unwrap();
        assert_eq!(presets[0].settings, settings);
        let json = serde_json::to_string(&presets).unwrap();
        let restored: Vec<NamedPreset> = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, presets);
        presets.retain(|p| p.name != "My NVIDIA preset");
        assert!(presets.is_empty());
    }

    #[test]
    fn preset_storage_round_trip_and_failure_keep_existing_presets() {
        let dir = std::env::temp_dir().join(format!("noni-preset-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = BananaApp::new(egui::Context::default());
        app.settings = crate::settings::UiSettings::in_file(dir.join("interface.json"));
        let mut dialog = RenderDialog::new(&app);
        let presets = vec![NamedPreset {
            name: "CBR 1440p".into(),
            settings: SimpleSettings {
                cbr: true,
                ..Default::default()
            },
        }];
        assert!(dialog.store_presets(&mut app, presets.clone()));
        let json = std::fs::read_to_string(dir.join("interface.json")).unwrap();
        let restored: crate::settings::UiSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.render.simple_presets, presets);
        // A file cannot be a parent folder: fail without losing the selection.
        app.settings =
            crate::settings::UiSettings::in_file(dir.join("interface.json/blocked.json"));
        app.settings.render.simple_presets = presets.clone();
        assert!(!dialog.store_presets(&mut app, Vec::new()));
        assert_eq!(dialog.simple_presets, presets);
        assert_eq!(app.settings.render.simple_presets, presets);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn estimate_uses_video_audio_and_selected_duration() {
        assert!((estimated_mb(60.0, 8_000_000, 192) - 61.44).abs() < 0.001);
        assert_eq!(estimated_mb(0.0, 8_000_000, 192), 0.0);
        assert_eq!(duration_text(557.9), "00:09:17");
        assert_eq!(duration_text(3661.0), "01:01:01");
    }

    #[test]
    fn old_saved_settings_default_to_vbr() {
        let mut json = serde_json::to_value(SimpleSettings::default()).unwrap();
        json.as_object_mut().unwrap().remove("cbr");
        let restored: SimpleSettings = serde_json::from_value(json).unwrap();
        assert!(!restored.cbr);
    }
}
