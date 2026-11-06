//! The everyday MP4 export dialog. The renderer chooses the available backend.

use bv_core::{FrameRate, Tick, TimeRange};
use bv_engine::{AudioParams, Encoder, FileFormat, RenderOptions, RenderSettings, VideoParams};
use serde::{Deserialize, Serialize};

use super::{Action, RenderDialog, templates};
use crate::app::BananaApp;
use crate::egui::{self, Align, Layout, RichText, Ui, vec2};
use crate::i18n::tr;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct SimpleSettings {
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

impl Default for SimpleSettings {
    fn default() -> Self {
        Self {
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
    pub(super) fn show_simple(&mut self, ui: &mut Ui, app: &BananaApp) -> Action {
        ui.set_min_width(455.0);
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
                let quality = |i: usize| {
                    [
                        tr("Smaller file"),
                        tr("Recommended"),
                        tr("Higher quality"),
                        tr("Custom"),
                    ][i.min(3)]
                    .to_string()
                };
                choose(
                    ui,
                    tr("Quality"),
                    &mut simple.quality,
                    (0..4).map(|i| (i, quality(i))),
                    quality,
                );
                let bitrate = simple.bitrate(
                    simple.size(project.settings.output_size()),
                    simple.rate.unwrap_or(project.settings.frame_rate),
                );
                ui.label(tr("Bit rate:"));
                if simple.quality == 3 {
                    ui.add(
                        egui::DragValue::new(&mut simple.video_kbps)
                            .range(750..=150_000)
                            .suffix(" kbps · VBR"),
                    );
                } else {
                    ui.label(format!("{} kbps · VBR", bitrate / 1000));
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
        let ready = self.simple_render_settings(app).is_some();
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let button = egui::Button::new(RichText::new(tr("Export")).strong())
                .fill(crate::theme::button_highlight())
                .min_size(vec2(96.0, 30.0));
            if ui.add_enabled(ready, button).clicked() {
                Action::Render
            } else if ui.button(tr("Cancel")).clicked()
                || ui.input(|i| i.key_pressed(egui::Key::Escape))
            {
                Action::Close
            } else {
                Action::None
            }
        })
        .inner
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
                max_bitrate: bitrate.saturating_mul(3) / 2,
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
