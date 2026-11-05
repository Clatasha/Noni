//! Sticker packs and locally imported artwork. Once used, stickers are normal
//! project media so timeline edits, preview transforms, and export share a path.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};

use crate::app::{BananaApp, MediaDrag};
use crate::egui::{self, Color32, ColorImage, Rect, TextureHandle, vec2};
use crate::i18n::tr;
use crate::theme;
use bv_timeline::TrackKind;

const MAX_PNG_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PNG_SIDE: u32 = 4096;
const CELL_WIDTH: f32 = 98.0;
const ROW_HEIGHT: f32 = 108.0;
const THUMB_SIDE: f32 = 80.0;

// Clatasha Studio's original packs, rasterized with their SVG aspect ratios.
// https://github.com/Clatasha/Clatasha-Studio/tree/main/assets/stickers
const BACKGROUNDS: &[u8] = include_bytes!("../../assets/stickers/background-shapes.zip");
const YOUTUBE: &[u8] = include_bytes!("../../assets/stickers/youtube.zip");

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
enum Pack {
    #[default]
    Backgrounds,
    YouTube,
    Custom,
}

impl Pack {
    const ALL: [Self; 3] = [Self::Backgrounds, Self::YouTube, Self::Custom];

    fn title(self) -> &'static str {
        match self {
            Self::Backgrounds => tr("Backgrounds"),
            Self::YouTube => tr("YouTube"),
            Self::Custom => tr("My Stickers"),
        }
    }

    fn folder(self) -> &'static str {
        match self {
            Self::Backgrounds => "Studio-1/Backgrounds",
            Self::YouTube => "Studio-1/YouTube",
            Self::Custom => "Custom",
        }
    }

    fn archive(self) -> Option<bv_zip::ZipReader<Cursor<&'static [u8]>>> {
        let data = match self {
            Self::Backgrounds => BACKGROUNDS,
            Self::YouTube => YOUTUBE,
            Self::Custom => return None,
        };
        bv_zip::ZipReader::new(Cursor::new(data)).ok()
    }
}

#[derive(Default)]
pub(crate) struct StickerPanel {
    pack: Pack,
    search: String,
    names: HashMap<Pack, Vec<String>>,
    textures: HashMap<(Pack, String), Option<TextureHandle>>,
}

pub(crate) fn ui(ui: &mut egui::Ui, app: &mut BananaApp) {
    let mut panel = std::mem::take(&mut app.stickers);
    panel.ui(ui, app);
    app.stickers = panel;
}

impl StickerPanel {
    fn ui(&mut self, ui: &mut egui::Ui, app: &mut BananaApp) {
        ui.horizontal_wrapped(|ui| {
            for pack in Pack::ALL {
                if ui.selectable_label(self.pack == pack, pack.title()).clicked() {
                    self.pack = pack;
                    ui.ctx().request_repaint();
                }
            }
            if ui.button(tr("Import Stickers…")).clicked()
                && let Some(paths) = rfd::FileDialog::new()
                    .add_filter(tr("PNG Image"), &["png"])
                    .pick_files()
            {
                for path in paths {
                    if let Err(error) = import_custom(&path) {
                        app.report_error(trf!("Could not import sticker: {}", error));
                    }
                }
                self.names.remove(&Pack::Custom);
                self.pack = Pack::Custom;
                self.search.clear();
            }
        });
        ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text(tr("Search stickers"))
                .desired_width(f32::INFINITY),
        );

        let pack = self.pack;
        let search = self.search.trim().to_lowercase();
        let filtered: Vec<_> = self
            .names
            .entry(pack)
            .or_insert_with(|| names(pack))
            .iter()
            .filter_map(|file| {
                let label = title(pack, file);
                label.to_lowercase().contains(&search).then(|| (file.clone(), label))
            })
            .collect();
        if filtered.is_empty() {
            super::hint(
                ui,
                if pack == Pack::Custom && search.is_empty() {
                    tr("Import transparent PNG files to add your own stickers.")
                } else {
                    tr("No stickers found.")
                },
            );
            return;
        }
        let space = ui.spacing().item_spacing.x;
        let columns = ((ui.available_width() + space) / (CELL_WIDTH + space)).max(1.0) as usize;
        egui::ScrollArea::vertical()
            .id_salt(("stickers", pack))
            .auto_shrink([false, false])
            .show_rows(ui, ROW_HEIGHT, filtered.len().div_ceil(columns), |ui, rows| {
                for row in rows {
                    ui.horizontal(|ui| {
                        for (file, label) in filtered.iter().skip(row * columns).take(columns) {
                            self.tile(ui, app, pack, file, label);
                        }
                    });
                }
            });
    }

    fn tile(&mut self, ui: &mut egui::Ui, app: &mut BananaApp, pack: Pack, file: &str, label: &str) {
        let (rect, response) =
            ui.allocate_exact_size(vec2(CELL_WIDTH, ROW_HEIGHT), egui::Sense::click_and_drag());
        let preview = Rect::from_center_size(
            rect.center_top() + vec2(0.0, 42.0),
            vec2(88.0, 80.0),
        );
        let painter = ui.painter();
        painter.rect_filled(
            preview,
            6.0,
            if response.hovered() { theme::hover() } else { theme::panel_dark() },
        );
        let texture = self.textures.entry((pack, file.to_owned())).or_insert_with(|| {
            let bytes = read_sticker(pack, file).ok()?;
            thumbnail(ui.ctx(), file, &bytes)
        });
        if let Some(texture) = texture {
            let source = texture.size_vec2();
            let image = Rect::from_center_size(preview.center(), source);
            painter.image(
                texture.id(),
                image,
                Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        let label_rect = Rect::from_min_max(
            rect.left_bottom() - vec2(0.0, 22.0),
            rect.right_bottom(),
        );
        let mut label_ui = ui.new_child(egui::UiBuilder::new().max_rect(label_rect));
        label_ui.set_clip_rect(label_ui.clip_rect().intersect(label_rect));
        label_ui.add(egui::Label::new(label).truncate().selectable(false));
        let response = response.on_hover_text(trf!(
            "{}\nClick to add at the playhead, or drag to the timeline.",
            label
        ));
        let dragging = response.drag_started_by(egui::PointerButton::Primary);
        if dragging || response.clicked() {
            match media_path(pack, file) {
                Ok(path) if dragging => {
                    app.media_window.drag_from = None;
                    app.media_drag = Some(MediaDrag::File(path));
                    app.media_drag_secondary = false;
                }
                Ok(path) => add_at_playhead(app, &path),
                Err(error) => app.report_error(trf!("Could not open sticker: {}", error)),
            }
        }
    }
}

fn thumbnail(ctx: &egui::Context, name: &str, png: &[u8]) -> Option<TextureHandle> {
    validate_png(png).ok()?;
    let image = bv_png::decode(png).ok()?;
    let src = image.to_rgba8();
    let (w, h) = (image.width as usize, image.height as usize);
    let scale = (THUMB_SIDE / w.max(h) as f32).min(1.0);
    let tw = (w as f32 * scale).round().max(1.0) as usize;
    let th = (h as f32 * scale).round().max(1.0) as usize;
    let mut rgba = Vec::with_capacity(tw * th * 4);
    for y in 0..th {
        for x in 0..tw {
            let from = ((y * h / th) * w + (x * w / tw)) * 4;
            rgba.extend_from_slice(&src[from..from + 4]);
        }
    }
    Some(ctx.load_texture(
        name,
        ColorImage::from_rgba_unmultiplied([tw, th], &rgba),
        egui::TextureOptions::LINEAR,
    ))
}

fn title(pack: Pack, file: &str) -> String {
    let stem = file.strip_suffix(".png").unwrap_or(file);
    match pack {
        Pack::Backgrounds => trf!("Background Shape {}", stem.trim_start_matches('0')),
        Pack::YouTube => stem
            .split_once('-')
            .map_or(stem, |(_, title)| title)
            .split('-')
            .filter_map(|word| {
                let mut chars = word.chars();
                let first = chars.next()?;
                Some(format!("{}{}", first.to_uppercase(), chars.as_str()))
            })
            .collect::<Vec<_>>()
            .join(" "),
        Pack::Custom => stem.to_owned(),
    }
}

fn folder(pack: Pack) -> io::Result<PathBuf> {
    bv_platform::config_dir()
        .map(|dir| dir.join("Noni").join("Stickers").join(pack.folder()))
        .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
}

fn names(pack: Pack) -> Vec<String> {
    if let Some(archive) = pack.archive() {
        return archive.entries().iter().map(|entry| entry.name.clone()).collect();
    }
    let mut names: Vec<_> = folder(pack)
        .ok()
        .and_then(|dir| fs::read_dir(dir).ok())
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|item| item.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|item| item.file_name().into_string().ok())
        .filter(|name| name.ends_with(".png"))
        .collect();
    names.sort_unstable();
    names
}

fn read_sticker(pack: Pack, file: &str) -> io::Result<Vec<u8>> {
    if pack == Pack::Custom {
        return read_png(&folder(pack)?.join(file));
    }
    let mut zip = pack.archive().ok_or(io::ErrorKind::InvalidData)?;
    let index = zip.find(file).ok_or(io::ErrorKind::NotFound)?;
    zip.read(index)
}

fn media_path(pack: Pack, file: &str) -> io::Result<PathBuf> {
    let dir = folder(pack)?;
    let path = dir.join(file);
    if pack == Pack::Custom || path.is_file() {
        return Ok(path);
    }
    let bytes = read_sticker(pack, file)?;
    fs::create_dir_all(dir)?;
    match write_new(&path, &bytes) {
        Ok(()) => Ok(path),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(path),
        Err(error) => Err(error),
    }
}

fn read_png(path: &Path) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(MAX_PNG_BYTES + 1).read_to_end(&mut bytes)?;
    validate_png(&bytes)?;
    Ok(bytes)
}

fn validate_png(bytes: &[u8]) -> io::Result<()> {
    let size_error = || io::Error::new(
        io::ErrorKind::InvalidData,
        tr("Stickers must be PNG files up to 4096 × 4096 pixels and 32 MB."),
    );
    if bytes.len() as u64 > MAX_PNG_BYTES {
        return Err(size_error());
    }
    let header = bv_png::read_header(bytes).map_err(|_| io::ErrorKind::InvalidData)?;
    if header.width == 0
        || header.height == 0
        || header.width > MAX_PNG_SIDE
        || header.height > MAX_PNG_SIDE
    {
        return Err(size_error());
    }
    Ok(())
}

fn import_custom(path: &Path) -> io::Result<()> {
    let bytes = read_png(path)?;
    bv_png::decode(&bytes).map_err(|_| io::ErrorKind::InvalidData)?;
    let dir = folder(Pack::Custom)?;
    fs::create_dir_all(&dir)?;
    let stem = path.file_stem().ok_or(io::ErrorKind::InvalidInput)?.to_string_lossy();
    let mut number = 1_u64;
    loop {
        let name = if number == 1 {
            format!("{stem}.png")
        } else {
            format!("{stem} ({number}).png")
        };
        let target = dir.join(name);
        match write_new(&target, &bytes) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                if read_png(&target).is_ok_and(|existing| existing == bytes) {
                    return Ok(());
                }
                number += 1;
            }
            result => return result,
        }
    }
}

fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let result = file.write_all(bytes);
    drop(file);
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

fn add_at_playhead(app: &mut BananaApp, path: &Path) {
    let at = app.mark_time();
    let length = app.settings.prefs.new_events().still_length;
    let name = tr("Stickers");
    app.as_one_undo(|app| {
        let Some(id) = app.import_paths(&[path.to_path_buf()]).first().copied() else {
            return;
        };
        let track = app.project().tracks.iter().find(|track| {
            track.kind == TrackKind::Video
                && !track.locked
                && !track.adjustment
                && !track.muted
                && (track.name == name || track.name == "Stickers")
                && track.clips.iter().all(|clip| clip.end() <= at || clip.start >= at + length)
        }).map(|track| track.id);
        let track = track.unwrap_or_else(|| {
            app.modify(|p| {
                let id = p.insert_track_at(TrackKind::Video, 0);
                if let Some(track) = p.tracks.iter_mut().find(|track| track.id == id) {
                    track.name = name.to_owned();
                }
                id
            })
        });
        app.place_media_streams(id, at, None, Some(track), true, false);
    });
}
