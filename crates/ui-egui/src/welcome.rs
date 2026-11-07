//! Noni home: project creation, recent projects and language selection.

use bv_core::FrameRate;

use crate::app::{BananaApp, Pending};
use crate::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, UiBuilder, pos2, vec2,
};
use crate::i18n::{Language, tr};
use crate::icons::{self, Icon};
use crate::theme;

/// The welcome screen's size and the header's and
/// footer's heights.
const SIZE: egui::Vec2 = vec2(860.0, 650.0);
const HEADER_H: f32 = 46.0;
const FOOTER_H: f32 = 30.0;
/// The New Project banner's height and the shape buttons'.
const BANNER_H: f32 = 152.0;
const SHAPE_H: f32 = 56.0;

/// The shape of a new project's frame, as VEGAS's welcome screen offers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Shape {
    #[default]
    Widescreen,
    Fullscreen,
    Portrait,
    Square,
    Scope,
}

impl Shape {
    const ALL: [Shape; 5] = [
        Shape::Widescreen,
        Shape::Fullscreen,
        Shape::Portrait,
        Shape::Square,
        Shape::Scope,
    ];

    fn name(self) -> &'static str {
        match self {
            Shape::Widescreen => tr("Widescreen (16:9)"),
            Shape::Fullscreen => tr("Fullscreen (4:3)"),
            Shape::Portrait => tr("Portrait (9:16)"),
            Shape::Square => tr("Square (1:1)"),
            Shape::Scope => tr("Scope (2.4:1)"),
        }
    }

    fn icon(self) -> Icon {
        match self {
            Shape::Widescreen => Icon::ShapeWide,
            Shape::Fullscreen => Icon::ShapeFull,
            Shape::Portrait => Icon::ShapePortrait,
            Shape::Square => Icon::ShapeSquare,
            Shape::Scope => Icon::ShapeScope,
        }
    }

    /// The frame sizes of HD, FullHD and UHD.
    fn sizes(self) -> [(u32, u32); 3] {
        match self {
            Shape::Widescreen => [(1280, 720), (1920, 1080), (3840, 2160)],
            Shape::Fullscreen => [(1280, 960), (1920, 1440), (4096, 3072)],
            Shape::Portrait => [(720, 1280), (1080, 1920), (2160, 3840)],
            Shape::Square => [(720, 720), (1080, 1080), (2160, 2160)],
            Shape::Scope => [(1280, 536), (2048, 858), (4096, 1716)],
        }
    }
}

/// The resolutions' names (names: not translated).
const SIZES: [&str; 3] = ["HD", "FullHD", "UHD"];

/// The frame rates offered, in frames a second (1/1.001 of them with Drop
/// Frame, as VEGAS offers them).
const RATES: [u32; 7] = [24, 25, 30, 48, 50, 60, 120];

/// The frame rate `n` frames a second, or 1/1.001 of it.
fn frame_rate(n: u32, drop_frame: bool) -> FrameRate {
    if drop_frame {
        FrameRate::new(n * 1000, 1001)
    } else {
        FrameRate::new(n, 1)
    }
}

/// "30p (29.970)".
fn rate_label(n: u32, drop_frame: bool) -> String {
    let r = frame_rate(n, drop_frame).as_f64();
    format!("{n}p ({})", crate::fmt::num(r, 3))
}

/// The welcome screen's state: open and the choices of a
/// new project.
#[derive(Debug)]
pub struct Welcome {
    pub open: bool,
    shape: Shape,
    /// HD, FullHD or UHD.
    size: usize,
    /// One of [`RATES`].
    rate: usize,
    drop_frame: bool,
    recent: RecentProjects,
}

impl Default for Welcome {
    /// Widescreen FullHD at 30p (29.970), as VEGAS proposes.
    fn default() -> Welcome {
        Welcome {
            open: false,
            shape: Shape::Widescreen,
            size: 1,
            rate: 2,
            drop_frame: true,
            recent: RecentProjects::default(),
        }
    }
}

impl Welcome {
    pub fn show(&mut self, ctx: &egui::Context, app: &mut BananaApp) {
        if !self.open {
            self.recent.paths.clear();
            return;
        }
        let mut open = true;
        let mut action = None;
        crate::floating::Window::new(tr("Welcome to Noni"))
            .id(egui::Id::new("boas-vindas"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| action = self.ui(ui, app));
        if let Some(a) = action {
            app.request(a);
            open = false;
        }
        self.open = open;
    }

    fn ui(&mut self, ui: &mut egui::Ui, app: &mut BananaApp) -> Option<Pending> {
        let (rect, _) = ui.allocate_exact_size(SIZE, Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme::panel_dark());
        // The header: the logo in the middle.
        let header = Rect::from_min_size(rect.min, vec2(rect.width(), HEADER_H));
        paint_logo(&painter, header.center(), 26.0, 20.0, theme::text_strong());
        let body = Rect::from_min_max(
            pos2(rect.left(), header.bottom()),
            pos2(rect.right(), rect.bottom() - FOOTER_H),
        );
        let content = body.shrink2(vec2(8.0, 0.0));
        let create = Rect::from_min_size(content.min, vec2(content.width(), 360.0));
        let mut action = self.new_page(ui, create);
        let recent = Rect::from_min_max(pos2(content.left(), create.bottom() + 6.0), content.max);
        if let Some(open) = self.recent.ui(ui, app, recent) {
            action = Some(open);
        }
        // "Show Welcome Screen on Startup".
        let footer = Rect::from_min_max(pos2(rect.left() + 8.0, body.bottom()), rect.max);
        ui.scope_builder(UiBuilder::new().max_rect(footer), |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let mut on = app.settings.show_welcome;
                if crate::widgets::checkbox(ui, &mut on, tr("Show Welcome Screen on Startup"))
                    .changed()
                {
                    app.settings.show_welcome = on;
                    let _ = app.settings.save();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let menu = ui.menu_button("     ", |ui| {
                        ui.set_min_width(180.0);
                        for &language in Language::ALL {
                            if ui
                                .selectable_label(
                                    crate::i18n::language() == language,
                                    language.native_name(),
                                )
                                .clicked()
                            {
                                app.set_language(language);
                                let _ = app.settings.save();
                                ui.close();
                            }
                        }
                    });
                    icons::paint(
                        ui.painter(),
                        menu.response.rect.shrink(3.0),
                        Icon::Globe,
                        theme::text(),
                    );
                    menu.response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tr("Language"))
                    });
                    menu.response.on_hover_text(tr("Language"));
                    if ui.button(tr("Open Project")).clicked() {
                        action = Some(Pending::Open);
                    }
                });
            });
        });
        action
    }

    /// New Project: the banner, the five shapes and the new project's
    /// size and rate.
    fn new_page(&mut self, ui: &mut egui::Ui, content: Rect) -> Option<Pending> {
        let painter = ui.painter_at(content);
        let banner = Rect::from_min_size(content.min, vec2(content.width(), BANNER_H));
        paint_banner(&painter.with_clip_rect(banner), banner);
        // The shapes, side by side.
        let row = Rect::from_min_size(
            pos2(content.left(), banner.bottom() + 4.0),
            vec2(content.width(), SHAPE_H),
        );
        let w = row.width() / Shape::ALL.len() as f32;
        for (i, shape) in Shape::ALL.into_iter().enumerate() {
            let r = Rect::from_min_size(
                pos2(row.left() + i as f32 * w, row.top()),
                vec2(w - 4.0, row.height()),
            );
            if tile(ui, r, shape.icon(), shape.name(), self.shape == shape, 28.0).clicked() {
                self.shape = shape;
            }
        }
        let form = Rect::from_min_max(pos2(content.left(), row.bottom() + 4.0), content.max);
        painter.rect_filled(form, 0.0, theme::panel());
        let mut action = None;
        ui.scope_builder(
            UiBuilder::new().max_rect(form.shrink2(vec2(12.0, 8.0))),
            |ui| {
                ui.label(RichText::new(tr("Create a new project")).strong());
                ui.add_space(4.0);
                let sizes = self.shape.sizes();
                // The labels' column as wide as the longer of them.
                let font = egui::TextStyle::Body.resolve(ui.style());
                let label_w = [tr("Resolution:"), tr("Framerate:")]
                    .iter()
                    .map(|t| {
                        ui.painter()
                            .layout_no_wrap(t.to_string(), font.clone(), Color32::WHITE)
                            .size()
                            .x
                    })
                    .fold(0.0, f32::max)
                    + 10.0;
                let label = |ui: &mut egui::Ui, text: &str| {
                    ui.allocate_ui_with_layout(
                        vec2(label_w, 20.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(label_w);
                            ui.label(text)
                        },
                    );
                };
                let full_w = ui.available_width() - label_w;
                ui.horizontal(|ui| {
                    label(ui, tr("Resolution:"));
                    let (w, h) = sizes[self.size];
                    egui::ComboBox::from_id_salt("boas-vindas-resolucao")
                        .width(full_w - 8.0)
                        .selected_text(format!("{} ({w} x {h})", SIZES[self.size]))
                        .show_ui(ui, |ui| {
                            for (i, (w, h)) in sizes.into_iter().enumerate() {
                                let text = format!("{} ({w} x {h})", SIZES[i]);
                                ui.selectable_value(&mut self.size, i, text);
                            }
                        });
                });
                ui.horizontal(|ui| {
                    label(ui, tr("Framerate:"));
                    let drop = self.drop_frame;
                    egui::ComboBox::from_id_salt("boas-vindas-taxa")
                        .width(full_w * 0.66 - 8.0)
                        .selected_text(rate_label(RATES[self.rate], drop))
                        .show_ui(ui, |ui| {
                            for (i, n) in RATES.into_iter().enumerate() {
                                ui.selectable_value(&mut self.rate, i, rate_label(n, drop));
                            }
                        });
                    let pull = |on: bool| {
                        if on {
                            tr("Drop Frame (1/1.001)")
                        } else {
                            tr("Full Frame")
                        }
                    };
                    egui::ComboBox::from_id_salt("boas-vindas-drop-frame")
                        .width(ui.available_width() - 2.0)
                        .selected_text(pull(self.drop_frame))
                        .show_ui(ui, |ui| {
                            for on in [false, true] {
                                ui.selectable_value(&mut self.drop_frame, on, pull(on));
                            }
                        });
                });
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.add_space(label_w);
                    let (width, height) = sizes[self.size];
                    let rate = frame_rate(RATES[self.rate], self.drop_frame);
                    let make = |advanced| Pending::NewSized {
                        width,
                        height,
                        rate,
                        advanced,
                    };
                    if ui
                        .add_sized([128.0, 22.0], egui::Button::new(tr("Create Project")))
                        .clicked()
                    {
                        action = Some(make(false));
                    }
                    if ui
                        .add_sized([128.0, 22.0], egui::Button::new(tr("Advanced Settings…")))
                        .on_hover_text(tr(
                            "A new project with these settings, in Project Properties",
                        ))
                        .clicked()
                    {
                        action = Some(make(true));
                    }
                });
            },
        );
        action
    }
}

/// Metadata is read off the UI thread. The engine's existing thumbnail queue
/// decodes the first used picture; missing, audio-only and legacy projects
/// keep a placeholder and remain openable through the usual unsaved guard.
#[derive(Debug, Default)]
struct RecentProjects {
    paths: Vec<std::path::PathBuf>,
    pictures:
        std::collections::HashMap<std::path::PathBuf, (bv_timeline::MediaInfo, bv_core::Tick)>,
    loading: Option<std::sync::mpsc::Receiver<Vec<RecentPicture>>>,
}

type RecentPicture = (std::path::PathBuf, bv_timeline::MediaInfo, bv_core::Tick);

impl RecentProjects {
    fn ui(&mut self, ui: &mut egui::Ui, app: &mut BananaApp, content: Rect) -> Option<Pending> {
        if self.paths != app.settings.recent {
            self.paths = app.settings.recent.clone();
            self.pictures.clear();
            let paths = self.paths.clone();
            let ctx = ui.ctx().clone();
            let (send, receive) = std::sync::mpsc::sync_channel(1);
            self.loading = std::thread::Builder::new()
                .name("noni-recent-projects".into())
                .spawn(move || {
                    let pictures = paths
                        .iter()
                        .filter_map(|path| recent_picture(path))
                        .collect();
                    let _ = send.send(pictures);
                    ctx.request_repaint();
                })
                .ok()
                .map(|_| receive);
        }
        if let Some(receiver) = &self.loading {
            match receiver.try_recv() {
                Ok(pictures) => {
                    self.pictures = pictures
                        .into_iter()
                        .map(|(path, info, at)| (path, (info, at)))
                        .collect();
                    self.loading = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.loading = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        let mut action = None;
        ui.scope_builder(UiBuilder::new().max_rect(content), |ui| {
            ui.label(RichText::new(tr("Recent Projects")).strong());
            if self.paths.is_empty() {
                ui.label(
                    RichText::new(tr("Your recent projects will appear here."))
                        .color(theme::text_dim()),
                );
                return;
            }
            egui::ScrollArea::vertical()
                .id_salt("noni-recent-projects")
                .max_height(188.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let width = (ui.available_width() - 24.0) / 4.0;
                    egui::Grid::new("noni-recent-cards")
                        .num_columns(4)
                        .spacing(vec2(8.0, 8.0))
                        .show(ui, |ui| {
                            for (index, path) in self.paths.iter().enumerate() {
                                let (card, response) =
                                    ui.allocate_exact_size(vec2(width, 90.0), Sense::click());
                                let painter = ui.painter_at(card);
                                painter.rect_filled(
                                    card,
                                    4.0,
                                    if response.hovered() {
                                        theme::hover()
                                    } else {
                                        theme::panel()
                                    },
                                );
                                let picture = Rect::from_min_max(
                                    card.min + vec2(4.0, 4.0),
                                    pos2(card.right() - 4.0, card.bottom() - 24.0),
                                );
                                let thumbnail = self
                                    .pictures
                                    .get(path)
                                    .and_then(|(info, at)| app.engine.thumbnail(info, *at, 96));
                                if let Some(thumbnail) = thumbnail {
                                    let texture = app.textures.get(ui.ctx(), &thumbnail);
                                    let scale = (picture.width() / thumbnail.width as f32)
                                        .min(picture.height() / thumbnail.height as f32);
                                    let image = Rect::from_center_size(
                                        picture.center(),
                                        vec2(
                                            thumbnail.width as f32 * scale,
                                            thumbnail.height as f32 * scale,
                                        ),
                                    );
                                    painter.image(
                                        texture,
                                        image,
                                        Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
                                        Color32::WHITE,
                                    );
                                } else {
                                    icons::paint(
                                        &painter,
                                        Rect::from_center_size(picture.center(), vec2(30.0, 30.0)),
                                        Icon::File,
                                        theme::text_dim(),
                                    );
                                }
                                let name = path
                                    .file_stem()
                                    .unwrap_or(path.as_os_str())
                                    .to_string_lossy();
                                let label = painter.layout_no_wrap(
                                    name.into_owned(),
                                    FontId::proportional(12.0),
                                    theme::text(),
                                );
                                let label_painter = painter.with_clip_rect(Rect::from_min_max(
                                    pos2(card.left() + 6.0, picture.bottom()),
                                    card.max - vec2(6.0, 0.0),
                                ));
                                label_painter.galley(
                                    pos2(card.left() + 6.0, card.bottom() - 20.0),
                                    label,
                                    theme::text(),
                                );
                                response.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::Button,
                                        true,
                                        path.file_stem()
                                            .unwrap_or(path.as_os_str())
                                            .to_string_lossy(),
                                    )
                                });
                                if response.on_hover_text(path.display().to_string()).clicked() {
                                    action = Some(Pending::OpenPath(path.clone()));
                                }
                                if index % 4 == 3 {
                                    ui.end_row();
                                }
                            }
                        });
                });
        });
        action
    }
}

fn recent_picture(path: &std::path::Path) -> Option<RecentPicture> {
    use std::io::Read;
    const MAX_BYTES: u64 = 16 * 1024 * 1024;
    let file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_BYTES {
        return None;
    }
    let mut json = String::new();
    file.take(MAX_BYTES + 1).read_to_string(&mut json).ok()?;
    if json.len() as u64 > MAX_BYTES {
        return None;
    }
    let project = bv_timeline::Project::from_json(&json).ok()?;
    let (mut info, at) = project
        .tracks
        .iter()
        .flat_map(|track| &track.clips)
        .filter_map(|clip| {
            project
                .media(clip.media)
                .filter(|info| info.video.is_some())
                .map(|info| (clip, info))
        })
        .min_by_key(|(clip, _)| clip.start)
        .map(|(clip, info)| (info.clone(), clip.source_in))
        .or_else(|| {
            project
                .media
                .iter()
                .find(|info| info.video.is_some())
                .cloned()
                .map(|info| (info, bv_core::Tick::ZERO))
        })?;
    if let bv_timeline::MediaSource::File { path: source } = &mut info.source {
        let source_path = std::path::Path::new(source);
        if source_path.is_relative() {
            *source = path
                .parent()?
                .join(source_path)
                .to_string_lossy()
                .into_owned();
        }
    }
    Some((path.to_path_buf(), info, at))
}

/// A shape button: its
/// icon over its name, blue when chosen.
fn tile(
    ui: &mut egui::Ui,
    r: Rect,
    icon: Icon,
    name: &str,
    on: bool,
    icon_size: f32,
) -> egui::Response {
    let resp = ui.interact(r, ui.id().with(("boas-vindas-botao", name)), Sense::click());
    let painter = ui.painter();
    if on {
        painter.rect_filled(r, 0.0, theme::selection());
    } else if resp.hovered() {
        painter.rect_filled(r, 0.0, theme::hover());
    }
    let color = if on { Color32::WHITE } else { theme::text() };
    let icon_rect = Rect::from_center_size(
        pos2(r.center().x, r.top() + 8.0 + icon_size / 2.0),
        vec2(icon_size, icon_size),
    );
    icons::paint(painter, icon_rect, icon, color);
    let galley = painter.layout(
        name.to_string(),
        FontId::proportional(12.0),
        color,
        r.width() - 6.0,
    );
    let at = pos2(
        r.center().x - galley.size().x / 2.0,
        (icon_rect.bottom() + 6.0).min(r.bottom() - galley.size().y - 4.0),
    );
    painter.galley(at, galley, color);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, name));
    resp
}

/// Noni's logo and its name, centered on `center`.
fn paint_logo(painter: &egui::Painter, center: Pos2, icon: f32, text: f32, color: Color32) {
    let galley = painter.layout_no_wrap("Noni".to_string(), FontId::proportional(text), color);
    let width = icon + 8.0 + galley.size().x;
    let left = center.x - width / 2.0;
    icons::paint_logo_mark(
        painter,
        Rect::from_center_size(pos2(left + icon / 2.0, center.y), vec2(icon, icon)),
    );
    painter.galley(
        pos2(left + icon + 8.0, center.y - galley.size().y / 2.0),
        galley,
        color,
    );
}

/// A rectangle filled with a gradient between its corners' colors (top
/// left, top right, bottom right, bottom left).
fn gradient(painter: &egui::Painter, r: Rect, corners: [Color32; 4]) {
    let mut mesh = egui::Mesh::default();
    for (p, c) in [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
    ]
    .into_iter()
    .zip(corners)
    {
        mesh.colored_vertex(p, c);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

/// A soft round light of `color` at `center`.
fn glow(painter: &egui::Painter, center: Pos2, radius: f32, color: Color32, strength: f32) {
    let steps = 14;
    for i in 0..steps {
        let t = 1.0 - i as f32 / steps as f32;
        let a = (strength * 255.0 / steps as f32) as u8;
        painter.circle_filled(
            center,
            radius * t,
            Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), a),
        );
    }
}

/// The New Project banner, Noni's own: a dusk sky, warm lights, a
/// timeline's events rising to the right and the logo.
fn paint_banner(painter: &egui::Painter, r: Rect) {
    gradient(
        painter,
        r,
        [
            Color32::from_rgb(0x1B, 0x22, 0x46),
            Color32::from_rgb(0x4A, 0x24, 0x62),
            Color32::from_rgb(0x9A, 0x3D, 0x3A),
            Color32::from_rgb(0x22, 0x1F, 0x3E),
        ],
    );
    glow(
        painter,
        r.right_bottom() + vec2(-120.0, -10.0),
        240.0,
        Color32::from_rgb(0xFF, 0xB0, 0x40),
        0.35,
    );
    glow(
        painter,
        r.left_top() + vec2(150.0, 70.0),
        170.0,
        Color32::from_rgb(0x50, 0x80, 0xFF),
        0.25,
    );
    // A timeline's events: three tracks of colored blocks and the cursor.
    let colors = [
        Color32::from_rgba_unmultiplied(0x4C, 0x8D, 0xE0, 200),
        Color32::from_rgba_unmultiplied(0x3F, 0xB4, 0xC8, 200),
        Color32::from_rgba_unmultiplied(0x5E, 0xC2, 0x6A, 200),
    ];
    let lane_h = 22.0;
    let base = pos2(r.left() + 400.0, r.bottom() - 28.0 - 3.0 * (lane_h + 8.0));
    let blocks: [[(f32, f32); 3]; 3] = [
        [(0.0, 90.0), (96.0, 70.0), (172.0, 120.0)],
        [(30.0, 110.0), (146.0, 60.0), (212.0, 80.0)],
        [(0.0, 150.0), (156.0, 140.0), (300.0, 0.0)],
    ];
    for (row, (lane, color)) in blocks.iter().zip(colors).enumerate() {
        let y = base.y + row as f32 * (lane_h + 8.0);
        for &(x, w) in lane {
            if w > 0.0 {
                let b = Rect::from_min_size(pos2(base.x + x, y), vec2(w, lane_h));
                painter.rect_filled(b, 4.0, color);
            }
        }
    }
    let cursor_x = base.x + 140.0;
    painter.line_segment(
        [
            pos2(cursor_x, base.y - 12.0),
            pos2(cursor_x, base.y + 3.0 * (lane_h + 8.0)),
        ],
        Stroke::new(2.0, Color32::from_white_alpha(220)),
    );
    painter.add(egui::Shape::convex_polygon(
        vec![
            pos2(cursor_x - 6.0, base.y - 20.0),
            pos2(cursor_x + 6.0, base.y - 20.0),
            pos2(cursor_x + 6.0, base.y - 14.0),
            pos2(cursor_x, base.y - 9.0),
            pos2(cursor_x - 6.0, base.y - 14.0),
        ],
        Color32::from_white_alpha(230),
        Stroke::NONE,
    ));
    // The logo and what it is.
    let logo = pos2(r.left() + 46.0, r.center().y - 22.0);
    icons::paint_logo_mark(
        painter,
        Rect::from_min_size(logo - vec2(0.0, 40.0), vec2(84.0, 84.0)),
    );
    painter.text(
        logo + vec2(98.0, -6.0),
        Align2::LEFT_CENTER,
        "Noni",
        FontId::proportional(44.0),
        Color32::WHITE,
    );
    painter.text(
        logo + vec2(100.0, 30.0),
        Align2::LEFT_CENTER,
        tr("Video editor"),
        FontId::proportional(16.0),
        Color32::from_white_alpha(210),
    );
}

impl BananaApp {
    /// The welcome screen open on `page` ("closed": not shown, whatever
    /// the user's settings say), for screen tests.
    pub fn welcome_for_review(&mut self, page: &str) {
        let w = &mut self.menu_dialogs.welcome;
        w.open = page != "closed";
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_buttons_return_guarded_project_actions() {
        use egui_kittest::kittest::Queryable;
        let mut app = BananaApp::new(egui::Context::default());
        app.settings.recent = vec![std::path::PathBuf::from("/missing/Recent edit.noniproj")];
        let mut harness = egui_kittest::Harness::builder()
            .with_size(vec2(900.0, 700.0))
            .build_ui_state(
                |ui, (app, welcome, action): &mut (BananaApp, Welcome, Option<Pending>)| {
                    if let Some(pending) = welcome.ui(ui, app) {
                        *action = Some(pending);
                    }
                },
                (app, Welcome::default(), None),
            );
        harness.get_by_label("Open Project").click();
        harness.run();
        assert!(matches!(harness.state_mut().2.take(), Some(Pending::Open)));
        harness.get_by_label("Recent edit").click();
        harness.run();
        assert!(
            matches!(harness.state_mut().2.take(), Some(Pending::OpenPath(path)) if path.ends_with("Recent edit.noniproj"))
        );
        harness.get_by_label("Create Project").click();
        harness.run();
        assert!(matches!(
            harness.state_mut().2.take(),
            Some(Pending::NewSized {
                width: 1920,
                height: 1080,
                advanced: false,
                ..
            })
        ));
        harness.get_by_label("Advanced Settings…").click();
        harness.run();
        assert!(matches!(
            harness.state_mut().2.take(),
            Some(Pending::NewSized { advanced: true, .. })
        ));
    }

    #[test]
    fn home_layout_review() {
        let mut app = BananaApp::new(egui::Context::default());
        let mut welcome = Welcome::default();
        crate::patch_review::capture("home-empty", vec2(900.0, 700.0), |ui| {
            welcome.ui(ui, &mut app);
        });
        app.settings.recent = (1..=12)
            .map(|n| std::path::PathBuf::from(format!("/missing/Project {n}.noniproj")))
            .collect();
        crate::patch_review::capture("home-recent", vec2(900.0, 700.0), |ui| {
            welcome.ui(ui, &mut app);
        });
    }

    #[test]
    fn recent_thumbnail_metadata_handles_bad_files_and_resolves_sources() {
        let dir = std::env::temp_dir().join(format!("noni-home-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("project.noniproj");
        assert!(recent_picture(&path).is_none());
        std::fs::write(&path, "broken project").unwrap();
        assert!(recent_picture(&path).is_none());
        let mut app = BananaApp::new(egui::Context::default());
        app.load_demo();
        let mut project = app.project().clone();
        let first = project
            .tracks
            .iter()
            .flat_map(|track| &track.clips)
            .find(|clip| {
                project
                    .media(clip.media)
                    .is_some_and(|info| info.video.is_some())
            })
            .unwrap()
            .clone();
        let info = project
            .media
            .iter_mut()
            .find(|info| info.id == first.media)
            .unwrap();
        info.source = bv_timeline::MediaSource::File {
            path: "clip.mp4".into(),
        };
        std::fs::write(&path, project.to_json()).unwrap();
        let (_, info, at) = recent_picture(&path).unwrap();
        assert_eq!(at, first.source_in);
        assert_eq!(
            info.source,
            bv_timeline::MediaSource::File {
                path: dir.join("clip.mp4").to_string_lossy().into_owned()
            }
        );
        project.tracks.clear();
        project.media.clear();
        std::fs::write(&path, project.to_json()).unwrap();
        assert!(recent_picture(&path).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_shapes_offer_vegas_sizes_and_rates() {
        assert_eq!(Shape::Widescreen.sizes()[1], (1920, 1080));
        assert_eq!(Shape::Scope.sizes()[2], (4096, 1716));
        assert_eq!(Shape::Portrait.sizes()[0], (720, 1280));
        assert_eq!(frame_rate(30, true), FrameRate::FPS_29_97);
        assert_eq!(frame_rate(25, false), FrameRate::FPS_25);
        crate::i18n::set_language(Language::English);
        assert_eq!(rate_label(30, true), "30p (29.970)");
        assert_eq!(rate_label(25, true), "25p (24.975)");
        assert_eq!(rate_label(120, false), "120p (120.000)");
        // VEGAS's: Widescreen FullHD at 30p (29.970).
        let w = Welcome::default();
        assert_eq!(
            (w.shape, w.size, RATES[w.rate], w.drop_frame),
            (Shape::Widescreen, 1, 30, true)
        );
    }
}
