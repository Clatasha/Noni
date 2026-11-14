//! Text inspector and a draft editor for the built-in title generator.

use bv_core::Tick;
use bv_ofx::{Context, ParamInfo, Value};
use bv_timeline::{Clip, ClipId, Generator, MediaId, MediaInfo, MediaSource, PluginFx, Transform};

use crate::app::BananaApp;
use crate::egui::{self, Color32, RichText, Ui, vec2};
use crate::i18n::tr;
use crate::ofx::{EditorKey, EditorOwner, FxPlace};

pub(crate) fn title(media: &MediaInfo) -> Option<&PluginFx> {
    match &media.source {
        MediaSource::Generated(Generator::Plugin(fx)) if fx.id == bv_engine::plugins::TITLES_ID => {
            Some(fx)
        }
        _ => None,
    }
}

fn write(project: &mut bv_timeline::Project, media: MediaId, fx: PluginFx) {
    if let Some(info) = project.media.iter_mut().find(|info| info.id == media)
        && title(info).is_some()
    {
        info.source = MediaSource::Generated(Generator::Plugin(Box::new(fx)));
    }
}

struct Controls<'a> {
    key: EditorKey,
    infos: &'a [ParamInfo],
    place: &'a FxPlace,
    fx: PluginFx,
    touched: Vec<String>,
}

impl<'a> Controls<'a> {
    fn new(fx: &PluginFx, infos: &'a [ParamInfo], place: &'a FxPlace, media: MediaId) -> Self {
        Self {
            key: EditorKey {
                owner: EditorOwner::Generator(media),
                index: 0,
                id: fx.id.clone(),
            },
            infos,
            place,
            fx: fx.clone(),
            touched: Vec::new(),
        }
    }

    fn get(&self, name: &str) -> Value {
        self.infos
            .iter()
            .find(|p| p.name == name)
            .map_or(Value::None, |p| {
                crate::ofx::value_now(p, &self.fx, self.place)
            })
    }

    fn set(&mut self, name: &str, value: Value) {
        if let Some(info) = self.infos.iter().find(|p| p.name == name)
            && crate::ofx::value_now(info, &self.fx, self.place) != value
        {
            crate::ofx::set_now(info, &mut self.fx, self.place, value);
            self.touched.push(name.to_owned());
        }
    }

    fn num(&self, name: &str) -> f64 {
        self.get(name).num(0)
    }
    fn set_num(&mut self, name: &str, value: f64) {
        self.set(name, Value::Num(vec![value]));
    }
    fn flag(&self, name: &str) -> bool {
        self.num(name) >= 0.5
    }
    fn set_flag(&mut self, name: &str, on: bool) {
        self.set_num(name, f64::from(u8::from(on)));
    }

    fn text_box(&mut self, ui: &mut Ui) {
        let mut text = self.get("text").text().to_owned();
        let edit = egui::TextEdit::multiline(&mut text)
            .desired_width(f32::INFINITY)
            .desired_rows(3)
            .show(ui);
        let selection = crate::ofx::rich_text_selection(&self.key, "text");
        if let Some(range) = edit.cursor_range.filter(|range| !range.is_empty()) {
            let sorted = range.as_sorted_char_range();
            ui.data_mut(|d| d.insert_temp(selection, (sorted.start, sorted.end)));
        } else {
            ui.data_mut(|d| d.remove::<(usize, usize)>(selection));
        }
        if edit.response.changed() {
            self.set("text", Value::Text(text));
        }
    }

    fn typography(&mut self, ui: &mut Ui, compact: bool) {
        let mut fields = |ui: &mut Ui| {
            if let Some(info) = self.infos.iter().find(|p| p.name == "font") {
                let font = self.get("font").text().to_owned();
                let mut picked = None;
                egui::ComboBox::from_id_salt("font-family")
                    .width(if compact {
                        148.0
                    } else {
                        (ui.available_width() * 0.49).clamp(80.0, 185.0)
                    })
                    .selected_text(&font)
                    .truncate()
                    .show_ui(ui, |ui| {
                        for (name, value) in info.options.iter().zip(&info.option_values) {
                            if ui.selectable_label(*value == font, name).clicked() {
                                picked = Some(value.clone());
                            }
                        }
                    })
                    .response
                    .on_hover_text(tr("Font"));
                if let Some(font) = picked {
                    self.set("font", Value::Text(font));
                }
            }
            let mut size = self.num("size");
            if ui
                .add(
                    egui::DragValue::new(&mut size)
                        .range(1.0..=2000.0)
                        .speed(0.5)
                        .fixed_decimals(0),
                )
                .on_hover_text(tr("Size"))
                .changed()
            {
                self.set_num("size", size);
            }
            self.color(ui, "textColor", tr("Text Color"));
            if compact {
                ui.separator();
                self.emphasis(ui, 24.0);
            }
        };
        if compact {
            ui.horizontal(|ui| {
                ui.spacing_mut().interact_size.y = 28.0;
                fields(ui);
            });
        } else {
            ui.horizontal_wrapped(fields);
        }
        if !compact {
            ui.horizontal(|ui| {
                let width =
                    ((ui.available_width() - 2.0 * ui.spacing().item_spacing.x) / 3.0).max(20.0);
                self.emphasis(ui, width);
            });
        }
        if !compact {
            self.alignment(ui);
        }
    }

    fn emphasis(&mut self, ui: &mut Ui, width: f32) {
        for (id, label, mark) in [
            ("bold", tr("Bold"), RichText::new("B").strong()),
            ("italic", tr("Italic"), RichText::new("I").italics()),
            ("underline", tr("Underline"), RichText::new("U").underline()),
        ] {
            let enabled = self.flag(id);
            if ui
                .add_sized([width, 24.0], egui::Button::selectable(enabled, mark))
                .on_hover_text(label)
                .clicked()
            {
                self.set_flag(id, !enabled);
            }
        }
    }

    fn alignment(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let width =
                ((ui.available_width() - 3.0 * ui.spacing().item_spacing.x) / 4.0).max(18.0);
            for (i, label) in [tr("Left"), tr("Center"), tr("Right"), tr("Justify")]
                .into_iter()
                .enumerate()
            {
                let selected = self.num("justification") as usize == i;
                let (rect, response) =
                    ui.allocate_exact_size(vec2(width, 24.0), egui::Sense::click());
                let visuals = ui.style().interact_selectable(&response, selected);
                ui.painter()
                    .rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
                for (j, len) in [13.0, 8.0, 11.0, 7.0].into_iter().enumerate() {
                    let len = if i == 3 { 13.0 } else { len };
                    let x = rect.center().x
                        + match i {
                            0 => -6.5,
                            2 => 6.5 - len,
                            _ => -len * 0.5,
                        };
                    let y = rect.center().y - 5.0 + j as f32 * 3.0;
                    ui.painter().line_segment(
                        [egui::pos2(x, y), egui::pos2(x + len, y)],
                        visuals.fg_stroke,
                    );
                }
                response.widget_info(|| {
                    egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label)
                });
                if response.on_hover_text(label).clicked() {
                    self.set_num("justification", i as f64);
                }
            }
        });
    }

    fn color(&mut self, ui: &mut Ui, field: &str, label: &str) {
        let c = self.get(field);
        let mut rgba = std::array::from_fn(|i| c.num(i) as f32);
        if ui
            .color_edit_button_rgba_unmultiplied(&mut rgba)
            .on_hover_text(label)
            .changed()
        {
            self.set(field, Value::Num(rgba.into_iter().map(f64::from).collect()));
        }
    }

    fn number(&mut self, ui: &mut Ui, id: &str, label: &str, range: std::ops::RangeInclusive<f64>) {
        ui.label(label);
        let mut val = self.num(id);
        if ui
            .add(
                egui::DragValue::new(&mut val)
                    .range(range)
                    .speed(0.02)
                    .max_decimals(3),
            )
            .changed()
        {
            self.set_num(id, val);
        }
    }

    fn toggle(&mut self, ui: &mut Ui, name: &str, label: &str) {
        let mut on = self.flag(name);
        if ui.checkbox(&mut on, label).changed() {
            self.set_flag(name, on);
        }
    }

    fn presets(&mut self, ui: &mut Ui, app: &BananaApp) {
        ui.label(tr("Presets"));
        let presets = crate::tabs::generators::titles_presets_all(app);
        let columns = ((ui.available_width() + 5.0) / 49.0)
            .floor()
            .clamp(1.0, 6.0) as usize;
        egui::ScrollArea::vertical()
            .id_salt("title-presets")
            .max_height(151.0)
            .show(ui, |ui| {
                egui::Grid::new("title-style-presets")
                    .spacing([5.0, 5.0])
                    .show(ui, |ui| {
                        for (i, (name, g)) in presets.iter().enumerate() {
                            let Generator::Plugin(style) = g else {
                                continue;
                            };
                            let sample = Controls::new(style, self.infos, self.place, MediaId(0));
                            let color = |id: &str| {
                                let value = sample.get(id);
                                let channel =
                                    |i| (value.num(i).clamp(0.0, 1.0) * 255.0).round() as u8;
                                Color32::from_rgba_unmultiplied(
                                    channel(0),
                                    channel(1),
                                    channel(2),
                                    channel(3),
                                )
                            };
                            let fill = color("textColor");
                            let back = color("background");
                            let (r, resp) =
                                ui.allocate_exact_size(vec2(44.0, 44.0), egui::Sense::click());
                            let painter = ui.painter();
                            painter.rect_filled(
                                r,
                                5.0,
                                if back.a() > 0 {
                                    back
                                } else {
                                    Color32::from_rgb(16, 19, 24)
                                },
                            );
                            let font = egui::FontId::proportional(28.0);
                            if sample.num("outlineWidth") > 0.0 {
                                let edge = color("outlineColor");
                                for (x, y) in [(-1.5, 0.0), (1.5, 0.0), (0.0, -1.5), (0.0, 1.5)] {
                                    painter.text(
                                        r.center() + vec2(x, y),
                                        egui::Align2::CENTER_CENTER,
                                        "A",
                                        font.clone(),
                                        edge,
                                    );
                                }
                            }
                            if sample.flag("shadowEnable") {
                                painter.text(
                                    r.center() + vec2(2.0, 2.0),
                                    egui::Align2::CENTER_CENTER,
                                    "A",
                                    font.clone(),
                                    Color32::from_gray(100),
                                );
                            }
                            painter.text(r.center(), egui::Align2::CENTER_CENTER, "A", font, fill);
                            if resp.hovered() {
                                painter.rect_stroke(
                                    r,
                                    5.0,
                                    egui::Stroke::new(1.0, crate::theme::snap()),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            resp.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name)
                            });
                            if resp.on_hover_text(name).clicked() {
                                let selection = crate::ofx::rich_text_selection(&self.key, "text");
                                ui.data_mut(|d| d.remove::<(usize, usize)>(selection));
                                for field in [
                                    "font",
                                    "size",
                                    "bold",
                                    "italic",
                                    "underline",
                                    "textColor",
                                    "background",
                                    "outlineWidth",
                                    "outlineColor",
                                    "shadowEnable",
                                    "shadowColor",
                                    "shadowOffsetX",
                                    "shadowOffsetY",
                                    "shadowBlur",
                                ] {
                                    self.set(field, sample.get(field));
                                }
                            }
                            if (i + 1) % columns == 0 {
                                ui.end_row();
                            }
                        }
                    });
            });
    }

    fn layout(&mut self, ui: &mut Ui) {
        self.number(ui, "tracking", tr("Letter Spacing"), -1.0..=5.0);
        self.number(ui, "lineSpacing", tr("Line Height"), 0.0..=10.0);
        ui.label(tr("Alignment"));
        let selected = self.num("anchor") as usize;
        egui::Grid::new("text-area-anchor")
            .spacing([3.0, 3.0])
            .show(ui, |ui| {
                for (i, (symbol, label)) in [
                    ("↖", tr("Top Left")),
                    ("↑", tr("Top Center")),
                    ("↗", tr("Top Right")),
                    ("←", tr("Center Left")),
                    ("⊙", tr("Center")),
                    ("→", tr("Center Right")),
                    ("↙", tr("Bottom Left")),
                    ("↓", tr("Bottom Center")),
                    ("↘", tr("Bottom Right")),
                ]
                .into_iter()
                .enumerate()
                {
                    if ui
                        .add_sized(
                            [25.0, 23.0],
                            egui::Button::selectable(selected == i, symbol),
                        )
                        .on_hover_text(label)
                        .clicked()
                    {
                        // Anchor the text at the corresponding frame position.
                        self.set_num("anchor", i as f64);
                        self.set(
                            "location",
                            Value::Num(vec![
                                0.08 + 0.42 * (i % 3) as f64,
                                0.92 - 0.42 * (i / 3) as f64,
                            ]),
                        );
                    }
                    if i % 3 == 2 {
                        ui.end_row();
                    }
                }
            });
        self.toggle(ui, "autoFit", tr("Auto Fit"));
        if ui.button(tr("Fit box to text")).clicked() {
            self.set_num("boxWidth", 0.0);
        }
        if self.num("boxWidth") > 0.0 {
            let mut width = self.num("boxWidth") * 100.0;
            if ui
                .add(
                    egui::DragValue::new(&mut width)
                        .range(5.0..=100.0)
                        .suffix(" %"),
                )
                .on_hover_text(tr("Text Box Width"))
                .changed()
            {
                self.set_num("boxWidth", width / 100.0);
            }
        }
    }

    fn fill(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let background = self.get("background");
            let mut on = background.num(3) > 0.0;
            if ui.checkbox(&mut on, tr("Background")).changed() {
                self.set(
                    "background",
                    Value::Num(vec![
                        background.num(0),
                        background.num(1),
                        background.num(2),
                        if on { 0.8 } else { 0.0 },
                    ]),
                );
            }
            if on {
                self.color(ui, "background", tr("Background"));
            }
        });
        ui.horizontal(|ui| {
            let mut on = self.num("outlineWidth") > 0.0;
            if ui.checkbox(&mut on, tr("Outline")).changed() {
                self.set_num("outlineWidth", if on { 0.055 } else { 0.0 });
            }
            if on {
                self.color(ui, "outlineColor", tr("Outline Color"));
            }
        });
    }

    fn finish(mut self, ui: &Ui, original: &PluginFx) -> PluginFx {
        crate::ofx::rich_text(
            ui,
            &self.key,
            self.infos,
            original,
            &mut self.fx,
            &self.touched,
        );
        self.fx
    }
}

pub(crate) fn inspector(
    ui: &mut Ui,
    app: &mut BananaApp,
    fx: &PluginFx,
    place: &FxPlace,
    clip: &mut Clip,
    transform: &mut Transform,
) -> bool {
    let Some(info) = bv_ofx::registry().info(&fx.id) else {
        ui.label(tr("The title generator could not be loaded."));
        return false;
    };
    let Some(context) = info.context(Context::Generator) else {
        return false;
    };
    let mut edit = Controls::new(fx, &context.params, place, clip.media);
    let mut advanced = false;
    let tray_id = ui.id().with(("title-keyframes", clip.id.0));
    let mut keyframes = ui.data(|d| d.get_temp::<bool>(tray_id)).unwrap_or(false);
    let mut action = None;
    let mut seek = None;
    ui.add_enabled_ui(!clip.locked, |ui| {
        let compact_footer = ui.available_width() < 320.0;
        let footer = if compact_footer { 59.0 } else { 36.0 };
        let tray = if keyframes { 132.0 } else { 0.0 };
        egui::ScrollArea::vertical()
            .id_salt(("text-inspector", clip.id.0))
            .max_height((ui.available_height() - footer - tray).max(42.0))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                edit.text_box(ui);
                edit.typography(ui, false);
                ui.add_space(6.0);
                edit.presets(ui, app);
                egui::CollapsingHeader::new(tr("More Text Options")).show(ui, |ui| edit.layout(ui));
                egui::CollapsingHeader::new(tr("Transform"))
                    .default_open(true)
                    .show(ui, |ui| {
                        transform_controls(ui, &mut edit, transform);
                    });
                egui::CollapsingHeader::new(tr("Compositing"))
                    .default_open(true)
                    .show(ui, |ui| {
                        let mut opacity = clip.level * 100.0;
                        if crate::tabs::properties::value_slider(
                            ui,
                            tr("Opacity"),
                            &mut opacity,
                            0.0..=100.0,
                            0,
                            " %",
                        ) {
                            clip.level = opacity / 100.0;
                        }
                        edit.fill(ui);
                    });
            });
        if keyframes && crate::ofx::prepare_title_keys(&mut app.ofx, &edit.key, &edit.fx, place) {
            ui.horizontal_wrapped(|ui| {
                for (name, label) in [
                    ("scale", tr("Scale")),
                    ("location", tr("Position")),
                    ("textColor", tr("Text Color")),
                ] {
                    if ui
                        .small_button(format!("◇ {label}"))
                        .on_hover_text(tr("Add keyframe"))
                        .clicked()
                        && let Some(value) = bv_engine::plugins::plugin_value(&edit.get(name))
                    {
                        use bv_timeline::{KeyCurve, PluginKey, PluginParam};
                        let param =
                            edit.fx
                                .params
                                .entry(name.into())
                                .or_insert_with(|| PluginParam {
                                    value: value.clone(),
                                    keys: Vec::new(),
                                });
                        if !param.keys.iter().any(|k| k.at == place.time) {
                            let i = param.keys.partition_point(|k| k.at < place.time);
                            param
                                .keys
                                .insert(i, PluginKey::new(place.time, value, KeyCurve::Linear));
                        }
                    }
                }
            });
            let (next, at) =
                crate::ofx::keyframes(ui, &mut app.ofx, &edit.key, &edit.fx, place, 100.0);
            if let Some(next) = next {
                edit.fx = next;
            }
            seek = at;
        }
        ui.separator();
        let mut manage = |ui: &mut Ui| {
            advanced = ui
                .add(
                    egui::Button::new(RichText::new(tr("Advanced")).color(Color32::WHITE))
                        .fill(crate::theme::ACTION_BLUE),
                )
                .clicked();
            if ui
                .selectable_label(keyframes, tr("Keyframe Panel"))
                .clicked()
            {
                keyframes = !keyframes;
                ui.data_mut(|d| d.insert_temp(tray_id, keyframes));
            }
        };
        ui.horizontal(|ui| {
            if ui.button(tr("Reset")).clicked() {
                action = Some(false);
            }
            if ui.button(tr("Fit Text")).clicked() {
                action = Some(true);
            }
            if !compact_footer {
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    &mut manage,
                );
            }
        });
        if compact_footer {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), manage);
        }
    });
    let mut next = edit.finish(ui, fx);
    if let Some(fit) = action {
        // The manual recovery clears the crop that can still cut out the generator.
        clip.pan_crop = Default::default();
        let mut edit = Controls::new(&next, &context.params, place, clip.media);
        for name in [
            "anchor",
            "scale",
            "boxWidth",
            "location",
            "autoFit",
            "freePosition",
        ] {
            edit.fx.params.remove(name);
        }
        for (name, val) in [("anchor", 4.0), ("scale", 1.0), ("boxWidth", 0.0)] {
            edit.set_num(name, val);
        }
        edit.set("location", Value::Num(vec![0.5, 0.5]));
        edit.set_flag("autoFit", true);
        if !fit {
            edit.fx.params.retain(|key, _| key == "text");
            edit.set_flag("autoFit", true);
            clip.level = 1.0;
        }
        next = edit.fx;
    }
    let media = clip.media;
    if action.is_some() {
        let id = clip.id;
        let pan = clip.pan_crop.clone();
        let opacity = clip.level;
        app.modify(|p| {
            if next != *fx {
                write(p, media, next);
            }
            if let Some(c) = p.clip_mut(id) {
                c.pan_crop = pan;
                c.level = opacity;
            }
        });
    } else if next != *fx {
        ui.ctx().request_repaint();
        app.modify_merged(ui.id().with(("title-text", media.0)), |p| {
            write(p, media, next)
        });
    }
    if let Some(at) = seek {
        app.cursor = at;
    }
    if advanced {
        app.open_title_editor(clip.id);
    }
    action.is_some()
}

fn transform_controls(ui: &mut Ui, edit: &mut Controls<'_>, transform: &mut Transform) {
    let mut rotation = transform.rotation_deg;
    if crate::tabs::properties::value_slider(
        ui,
        tr("Rotate"),
        &mut rotation,
        -360.0..=360.0,
        2,
        "°",
    ) {
        transform.rotation_deg = rotation;
    }
    let mut scale = (edit.num("scale") * 100.0) as f32;
    if crate::tabs::properties::value_slider(ui, tr("Scale"), &mut scale, 1.0..=400.0, 1, " %") {
        edit.set_num("scale", f64::from(scale) / 100.0);
    }
    ui.label(tr("Position"));
    let location = edit.get("location");
    let size = [
        f64::from(edit.place.size.0.max(1)),
        f64::from(edit.place.size.1.max(1)),
    ];
    let mut xy = std::array::from_fn::<_, 2, _>(|i| (location.num(i) - 0.5) * size[i]);
    ui.horizontal_wrapped(|ui| {
        let mut changed = false;
        for (i, label) in ["X", "Y"].into_iter().enumerate() {
            ui.label(label);
            changed |= ui
                .add(
                    egui::DragValue::new(&mut xy[i])
                        .speed(1)
                        .suffix(" px")
                        .range(-size[i]..=size[i]),
                )
                .changed();
        }
        if changed {
            edit.set_flag("freePosition", true);
            edit.set(
                "location",
                Value::Num(vec![xy[0] / size[0] + 0.5, xy[1] / size[1] + 0.5]),
            );
        }
    });
}

/// A separate draft lets Cancel leave the title and timeline untouched.
pub(crate) struct Advanced {
    media: MediaId,
    original: PluginFx,
    fx: PluginFx,
    original_animation: [bv_timeline::ClipAnimation; 2],
    enter: bv_timeline::ClipAnimation,
    exit: bv_timeline::ClipAnimation,
    tab: AdvancedTab,
    at: Tick,
    playing: bool,
    zoom: f32,
    preset: Option<String>,
    picture: Option<TitlePicture>,
    picture_job: Option<TitleRender>,
}

impl std::fmt::Debug for Advanced {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdvancedText")
            .field("media", &self.media)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AdvancedTab {
    Preset,
    Customize,
    Animation,
}

#[derive(PartialEq)]
struct TitleFrame {
    fx: PluginFx,
    time: Tick,
    size: (u32, u32),
    duration: Tick,
    rate: bv_core::FrameRate,
}

struct TitlePicture {
    frame: TitleFrame,
    texture: Option<egui::TextureHandle>,
}

struct TitleRender {
    frame: TitleFrame,
    result: std::sync::mpsc::Receiver<Option<egui::ColorImage>>,
}

impl Advanced {
    pub(crate) fn new(app: &BananaApp, id: ClipId) -> Option<Self> {
        let c = app.project().clip(id)?;
        let fx = title(app.project().media(c.media)?)?.clone();
        Some(Self {
            media: c.media,
            original: fx.clone(),
            fx,
            enter: c.animation_in,
            exit: c.animation_out,
            original_animation: [c.animation_in, c.animation_out],
            tab: AdvancedTab::Customize,
            at: Tick::ZERO,
            playing: false,
            zoom: 1.0,
            preset: None,
            picture: None,
            picture_job: None,
        })
    }

    pub(crate) fn show(&mut self, ctx: &egui::Context, app: &mut BananaApp, id: ClipId) -> bool {
        let Some(clip) = app
            .project()
            .clip(id)
            .filter(|c| c.media == self.media && !c.locked)
            .cloned()
        else {
            return false;
        };
        if app.project().clip_track(id).is_none_or(|t| t.locked) {
            return false;
        }
        let Some(media) = app.project().media(self.media).cloned() else {
            return false;
        };
        if title(&media).is_none() {
            return false;
        }
        let Some(info) = bv_ofx::registry().info(&self.fx.id) else {
            return false;
        };
        let Some(context) = info.context(Context::Generator) else {
            return false;
        };
        let last = (clip.duration - app.frame_rate().frame_duration()).max(Tick::ZERO);
        if self.playing {
            let dt = ctx.input(|i| i.stable_dt).clamp(0.0, 0.1);
            self.at += Tick::from_seconds_f64(f64::from(dt));
            if self.at >= last {
                self.at = last;
                self.playing = false;
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        self.at = self.at.clamp(Tick::ZERO, last);
        let Some(mut place) = crate::ofx::generator_place(app.project(), id, clip.start + self.at)
        else {
            return false;
        };
        if let Some(v) = media.video {
            place.rate = v.frame_rate;
        }
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        let mut save = None;
        crate::floating::Window::new(tr("Advanced Text Edit"))
            .id(egui::Id::new("advanced-text-edit"))
            .default_size(vec2(1080.0, 610.0))
            .min_size(vec2(670.0, 440.0))
            .collapsible(false)
            .open(&mut open)
            .show(ctx, |ui| {
                let mut edit = Controls::new(&self.fx, &context.params, &place, media.id);
                let body = (ui.available_height() - 36.0).max(320.0);
                let right = (ui.available_width() * 0.31).clamp(205.0, 365.0);
                let left = (ui.available_width() - right - ui.spacing().item_spacing.x).max(300.0);
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        vec2(left, body),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(left);
                            edit.typography(ui, true);
                            self.preview(ui, &edit.fx, &place, &clip, (body - 185.0).max(112.0));
                            self.transport(ui, last, place.rate);
                            self.timeline(ui, edit.get("text").text(), clip.duration, place.rate);
                        },
                    );
                    ui.allocate_ui_with_layout(
                        vec2(right, body),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(right);
                            self.tab = crate::tabs::properties::tab_row(
                                ui,
                                "advanced-title-tabs",
                                &[
                                    (AdvancedTab::Preset, tr("Presets")),
                                    (AdvancedTab::Customize, tr("Customize")),
                                    (AdvancedTab::Animation, tr("Animation")),
                                ],
                                self.tab,
                            );
                            ui.separator();
                            egui::ScrollArea::vertical()
                                .id_salt("advanced-text-scroll")
                                .max_height((body - 44.0).max(100.0))
                                .auto_shrink([false, false])
                                .show(ui, |ui| match self.tab {
                                    AdvancedTab::Preset => edit.presets(ui, app),
                                    AdvancedTab::Customize => {
                                        ui.strong(tr("Input Text"));
                                        edit.text_box(ui);
                                        edit.alignment(ui);
                                        ui.separator();
                                        ui.strong(tr("Fill"));
                                        edit.color(ui, "textColor", tr("Text Color"));
                                        let mut alpha = edit.get("textColor").num(3) as f32 * 100.0;
                                        if crate::tabs::properties::value_slider(
                                            ui,
                                            tr("Opacity"),
                                            &mut alpha,
                                            0.0..=100.0,
                                            0,
                                            " %",
                                        ) {
                                            let rgb = edit.get("textColor");
                                            edit.set(
                                                "textColor",
                                                Value::Num(vec![
                                                    rgb.num(0),
                                                    rgb.num(1),
                                                    rgb.num(2),
                                                    f64::from(alpha) / 100.0,
                                                ]),
                                            );
                                        }
                                        edit.fill(ui);
                                        if edit.num("outlineWidth") > 0.0 {
                                            edit.number(
                                                ui,
                                                "outlineWidth",
                                                tr("Outline Width"),
                                                0.0..=1.0,
                                            );
                                        }
                                        edit.toggle(ui, "shadowEnable", tr("Shadow"));
                                        if edit.flag("shadowEnable") {
                                            edit.color(ui, "shadowColor", tr("Shadow Color"));
                                            edit.number(
                                                ui,
                                                "shadowBlur",
                                                tr("Shadow Blur"),
                                                0.0..=1.0,
                                            );
                                            edit.number(
                                                ui,
                                                "shadowOffsetX",
                                                tr("Shadow Offset X"),
                                                -1.0..=1.0,
                                            );
                                            edit.number(
                                                ui,
                                                "shadowOffsetY",
                                                tr("Shadow Offset Y"),
                                                -1.0..=1.0,
                                            );
                                        }
                                        egui::CollapsingHeader::new(tr("More Text Options"))
                                            .show(ui, |ui| edit.layout(ui));
                                    }
                                    AdvancedTab::Animation => {
                                        ui.strong(tr("In"));
                                        crate::tabs::properties::animation_tiles(
                                            ui,
                                            &mut self.enter,
                                            clip.duration,
                                        );
                                        ui.separator();
                                        ui.strong(tr("Out"));
                                        crate::tabs::properties::animation_tiles(
                                            ui,
                                            &mut self.exit,
                                            clip.duration,
                                        );
                                    }
                                });
                        },
                    );
                });
                let next = edit.finish(ui, &self.fx);
                if next != self.fx {
                    self.fx = next;
                    ui.ctx().request_repaint();
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(tr("Save as custom")).clicked() {
                        self.preset.get_or_insert_with(String::new);
                    }
                    if let Some(name) = &mut self.preset {
                        ui.add(
                            egui::TextEdit::singleline(name)
                                .desired_width(135.0)
                                .hint_text(tr("Preset Name")),
                        );
                        if ui
                            .add_enabled(!name.trim().is_empty(), egui::Button::new(tr("Save")))
                            .clicked()
                        {
                            save = Some(name.trim().to_string());
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        cancel = ui.button(tr("Cancel")).clicked();
                        apply = ui
                            .add(
                                egui::Button::new(RichText::new(tr("Apply")).color(Color32::WHITE))
                                    .fill(crate::theme::ACTION_BLUE),
                            )
                            .clicked();
                    });
                });
            });
        if let Some(name) = save {
            let generator = Generator::Plugin(Box::new(self.fx.clone()));
            let (plugin, _, _) = crate::tabs::generators::plugin_of(app, &generator);
            let presets = &mut app.settings.generator_presets;
            presets.retain(|p| p.plugin != plugin || p.name != name);
            presets.push(crate::settings::GeneratorPreset {
                plugin,
                name,
                generator,
            });
            if let Err(error) = app.settings.save() {
                app.report_error(error);
            }
            self.preset = None;
        }
        if apply && !cancel && open {
            let changed = self.fx != self.original;
            let media = self.media;
            let fx = self.fx.clone();
            let original = &self.original;
            let (enter, exit) = (self.enter, self.exit);
            let original_animation = self.original_animation;
            app.modify(|p| {
                if changed && let Some(mut current) = p.media(media).and_then(title).cloned() {
                    for name in original.params.keys().chain(fx.params.keys()) {
                        if original.params.get(name) != fx.params.get(name) {
                            if let Some(value) = fx.params.get(name) {
                                current.params.insert(name.clone(), value.clone());
                            } else {
                                current.params.remove(name);
                            }
                        }
                    }
                    write(p, media, current);
                }
                if let Some(c) = p.clip_mut(id) {
                    if enter != original_animation[0] {
                        c.animation_in = enter;
                    }
                    if exit != original_animation[1] {
                        c.animation_out = exit;
                    }
                }
            });
        }
        open && !apply && !cancel
    }

    fn transport(&mut self, ui: &mut Ui, last: Tick, rate: bv_core::FrameRate) {
        use crate::icons::Icon;
        ui.horizontal(|ui| {
            if crate::widgets::icon_button(ui, Icon::GoStart, tr("Go to Start")).clicked() {
                self.at = Tick::ZERO;
            }
            if crate::widgets::icon_button(
                ui,
                if self.playing {
                    Icon::Pause
                } else {
                    Icon::Play
                },
                if self.playing {
                    tr("Pause")
                } else {
                    tr("Play")
                },
            )
            .clicked()
            {
                if self.at >= last {
                    self.at = Tick::ZERO;
                }
                self.playing = !self.playing;
            }
            if crate::widgets::icon_button(ui, Icon::Stop, tr("Stop")).clicked() {
                self.at = Tick::ZERO;
                self.playing = false;
            }
            ui.label(bv_core::timecode::format(self.at, rate));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("+").on_hover_text(tr("Zoom In")).clicked() {
                    self.zoom = (self.zoom * 1.25).min(4.0);
                }
                if ui.small_button("−").on_hover_text(tr("Zoom Out")).clicked() {
                    self.zoom = (self.zoom / 1.25).max(0.25);
                }
                if ui.small_button(tr("Fit")).clicked() {
                    self.zoom = 1.0;
                }
            });
        });
    }

    fn timeline(&mut self, ui: &mut Ui, text: &str, duration: Tick, rate: bv_core::FrameRate) {
        let (rect, response) = ui.allocate_exact_size(
            vec2(ui.available_width().max(1.0), 66.0),
            egui::Sense::click_and_drag(),
        );
        if (response.clicked() || response.dragged())
            && let Some(pos) = response.interact_pointer_pos()
        {
            let fraction = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64;
            self.at = rate.snap_floor(Tick::from_seconds_f64(fraction * duration.as_seconds_f64()));
            self.playing = false;
        }
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 5.0, crate::theme::panel_dark());
        for i in 0..=5 {
            let x = rect.left() + rect.width() * i as f32 / 5.0;
            let t = Tick::from_seconds_f64(duration.as_seconds_f64() * i as f64 / 5.0);
            p.line_segment(
                [
                    egui::pos2(x, rect.top() + 16.0),
                    egui::pos2(x, rect.top() + 21.0),
                ],
                egui::Stroke::new(1.0, crate::theme::text_dim()),
            );
            if i < 5 {
                p.text(
                    egui::pos2(x + 3.0, rect.top() + 2.0),
                    egui::Align2::LEFT_TOP,
                    bv_core::timecode::format(t, rate),
                    egui::FontId::monospace(10.0),
                    crate::theme::text_dim(),
                );
            }
        }
        let bar = egui::Rect::from_min_max(
            rect.left_top() + vec2(1.0, 24.0),
            rect.right_top() + vec2(-1.0, 48.0),
        );
        p.rect_filled(bar, 4.0, Color32::from_rgb(98, 75, 168));
        p.text(
            bar.left_center() + vec2(7.0, 0.0),
            egui::Align2::LEFT_CENTER,
            text.lines().next().unwrap_or(""),
            egui::FontId::proportional(12.0),
            Color32::WHITE,
        );
        let x = rect.left()
            + rect.width()
                * (self.at.as_seconds_f64() / duration.as_seconds_f64().max(1e-9)).clamp(0.0, 1.0)
                    as f32;
        p.vline(
            x,
            rect.y_range(),
            egui::Stroke::new(1.0, Color32::from_rgb(251, 92, 107)),
        );
    }

    fn preview(&mut self, ui: &mut Ui, fx: &PluginFx, place: &FxPlace, clip: &Clip, height: f32) {
        let (area, _) = ui.allocate_exact_size(
            vec2(ui.available_width().max(1.0), height),
            egui::Sense::hover(),
        );
        let p = ui.painter_at(area);
        p.rect_filled(area, 5.0, Color32::from_gray(17));
        let frame = TitleFrame {
            fx: fx.clone(),
            time: if fx.params.values().any(|p| !p.keys.is_empty()) {
                place.rate.snap_floor(place.time)
            } else {
                Tick::ZERO
            },
            size: place.size,
            duration: place.duration,
            rate: place.rate,
        };
        if let Some(job) = &self.picture_job {
            use std::sync::mpsc::TryRecvError;
            let image = match job.result.try_recv() {
                Ok(image) => Some(image),
                Err(TryRecvError::Disconnected) => Some(None),
                Err(TryRecvError::Empty) => None,
            };
            if let Some(image) = image {
                let job = self.picture_job.take().unwrap();
                let texture = image.map(|image| {
                    let options = egui::TextureOptions::LINEAR;
                    if let Some(mut texture) = self.picture.take().and_then(|pic| pic.texture) {
                        texture.set(image, options);
                        texture
                    } else {
                        ui.ctx()
                            .load_texture("advanced-title-picture", image, options)
                    }
                });
                self.picture = Some(TitlePicture {
                    frame: job.frame,
                    texture,
                });
            }
        }
        if self.picture_job.is_none() && self.picture.as_ref().is_none_or(|pic| pic.frame != frame)
        {
            let (tx, result) = std::sync::mpsc::channel();
            let input = frame.fx.clone();
            let mut place = *place;
            place.time = frame.time;
            let ctx = ui.ctx().clone();
            if std::thread::Builder::new()
                .name("noni-title-preview".into())
                .stack_size(bv_ofx::PLUGIN_STACK)
                .spawn(move || {
                    let _ = tx.send(render_title(&input, &place));
                    ctx.request_repaint();
                })
                .is_ok()
            {
                self.picture_job = Some(TitleRender { frame, result });
            }
        }
        let aspect = place.size.0.max(1) as f32 / place.size.1.max(1) as f32;
        let height = (area.width() / aspect).min(area.height());
        let canvas =
            egui::Rect::from_center_size(area.center(), vec2(height * aspect, height) * self.zoom);
        p.rect_filled(canvas, 0.0, Color32::BLACK);
        if let Some(texture) = self.picture.as_ref().and_then(|pic| pic.texture.as_ref()) {
            let mut draft = clip.clone();
            draft.animation_in = self.enter;
            draft.animation_out = self.exit;
            let pose = bv_timeline::scene::clip_animation(&draft, clip.start + self.at);
            p.with_clip_rect(canvas).image(
                texture.id(),
                egui::Rect::from_center_size(
                    canvas.center()
                        + vec2(pose.offset[0] as f32, pose.offset[1] as f32) * canvas.size(),
                    canvas.size() * pose.scale as f32,
                ),
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                Color32::WHITE.gamma_multiply(pose.opacity.clamp(0.0, 1.0)),
            );
        }
    }
}

fn render_title(fx: &PluginFx, place: &FxPlace) -> Option<egui::ColorImage> {
    let plugin = bv_ofx::registry().plugin(&fx.id).ok()?;
    let (width, height) = (place.size.0.max(1), place.size.1.max(1));
    let scale = (640.0 / f64::from(width))
        .min(400.0 / f64::from(height))
        .min(1.0);
    let width_out = (f64::from(width) * scale).round().max(1.0) as u32;
    let height_out = (f64::from(height) * scale).round().max(1.0) as u32;
    let instance = bv_ofx::Instance::new(
        &plugin,
        Context::Generator,
        &bv_ofx::Setup {
            frame_rate: place.rate.as_f64(),
            duration: bv_engine::plugins::frames(place.duration, place.rate),
            size: (f64::from(width), f64::from(height)),
            pixel_aspect: 1.0,
            interactive: true,
        },
    )
    .ok()?;
    bv_engine::plugins::sync_values(&instance, &fx.params, place.rate);
    let picture = instance
        .render(&bv_ofx::RenderRequest {
            time: bv_engine::plugins::frames(place.time, place.rate),
            width: width_out,
            height: height_out,
            scale,
            draft: true,
            interactive: true,
            sequential: false,
            timeline: None,
        })
        .ok()?;
    let pixels: Vec<u8> = picture
        .data
        .iter()
        .flat_map(|px| {
            let a = px[3].clamp(0.0, 1.0);
            let display = |v: f32| {
                let v = if a > 0.0 {
                    (v / a).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let srgb = if v <= 0.0031308 {
                    12.92 * v
                } else {
                    1.055 * v.powf(1.0 / 2.4) - 0.055
                };
                (srgb * 255.0).round() as u8
            };
            [
                display(px[0]),
                display(px[1]),
                display(px[2]),
                (a * 255.0).round() as u8,
            ]
        })
        .collect();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [picture.width as usize, picture.height as usize],
        &pixels,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_overlay_drags_horizontally_to_the_canvas_edge() {
        use bv_ofx::interact::{Mark, PenAction, View};
        let mut app = BananaApp::new(egui::Context::default());
        app.add_generator(
            crate::tabs::generators::titles_generator().unwrap(),
            Tick::ZERO,
            None,
        );
        let clip = app.project().clips().next().unwrap().1;
        let fx = title(app.project().media(clip.media).unwrap()).unwrap();
        let place = crate::ofx::generator_place(app.project(), clip.id, Tick::ZERO).unwrap();
        let plugin = bv_ofx::registry().plugin(&fx.id).unwrap();
        let size = (f64::from(place.size.0), f64::from(place.size.1));
        let instance = bv_ofx::Instance::new(
            &plugin,
            Context::Generator,
            &bv_ofx::Setup {
                size,
                interactive: true,
                frame_rate: place.rate.as_f64(),
                duration: bv_engine::plugins::frames(place.duration, place.rate),
                pixel_aspect: 1.0,
            },
        )
        .unwrap();
        bv_engine::plugins::sync_values(&instance, &fx.params, place.rate);
        let view = View {
            time: 0.0,
            render_scale: 1.0,
            pixel_scale: [1.0, 1.0],
            background: [0.0; 3],
            viewport: [place.size.0 as i32, place.size.1 as i32],
            to_frame: None,
        };
        let mark = || {
            instance
                .overlay_draw(&view)
                .into_iter()
                .find_map(|mark| match mark {
                    Mark::Strip {
                        points,
                        closed: true,
                        ..
                    } if points.len() == 4 => Some(points),
                    _ => None,
                })
                .unwrap()
        };
        let points = mark();
        let center = [
            (points[0][0] + points[2][0]) * 0.5,
            (points[0][1] + points[2][1]) * 0.5,
        ];
        let send = |action, p| instance.overlay_pen(action, &view, p, [0, 0], 1.0);
        assert!(send(PenAction::Down, center).taken);
        let edge = [0.0, center[1]];
        let moved = send(PenAction::Motion, edge);
        assert!(moved.changed.iter().any(|name| name == "freePosition"));
        assert!(mark()[0][0] < 0.0);
        assert!(send(PenAction::Up, edge).taken);
    }

    #[test]
    fn title_preview_and_text_panels_use_the_same_generator() {
        let mut app = BananaApp::new(egui::Context::default());
        app.welcome_for_review("closed");
        let generator = crate::tabs::generators::titles_generator().unwrap();
        app.add_generator(generator, Tick::ZERO, None);
        let id = app.project().clips().next().unwrap().1.id;
        app.select_clip(id, false);
        let place = crate::ofx::generator_place(app.project(), id, Tick::ZERO).unwrap();
        let clip = app.project().clip(id).unwrap().clone();
        let fx = title(app.project().media(clip.media).unwrap())
            .unwrap()
            .clone();
        assert_eq!(
            fx.params["autoFit"].value,
            bv_timeline::PluginValue::Num(vec![1.0])
        );
        let image = render_title(&fx, &place).expect("title preview");
        assert!(image.pixels.iter().any(|color| color.a() > 0));
        assert_eq!(image.pixels[0].a(), 0);
        let before = app.project().clone();
        for (name, width) in [("text-basic", 455.0), ("text-narrow", 272.0)] {
            crate::patch_review::capture(name, vec2(width, 785.0), |ui| {
                crate::tabs::properties::ui(ui, &mut app);
            });
        }
        let mut draft = Advanced::new(&app, id).unwrap();
        crate::patch_review::capture("text-advanced", vec2(1160.0, 700.0), |ui| {
            draft.show(ui.ctx(), &mut app, id);
        });
        assert_eq!(app.project(), &before);
    }
}
