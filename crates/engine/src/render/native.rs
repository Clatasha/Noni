//! Runs the packaged native export worker for timelines it can reproduce.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use bv_core::{FrameRate, Tick};
use bv_timeline::{Clip, MediaSource, Project, Track, TrackKind};
use serde::Serialize;

use super::{FileFormat, RenderProgress, RenderSettings};

#[derive(Serialize)]
struct Request {
    path: PathBuf,
    width: u32,
    height: u32,
    fps: u32,
    hevc: bool,
    bitrate: u32,
    audio_bitrate: u32,
    frames: u64,
    clips: Vec<Segment>,
}

#[derive(Serialize)]
struct Segment {
    source: PathBuf,
    start: f64,
    end: f64,
    audio: bool,
}

fn plain_track(track: &Track) -> bool {
    track.level == 1.0
        && track.pan == 0.0
        && track.pan_mode == Default::default()
        && track.composite_mode == Default::default()
        && track.parent_mode == Default::default()
        && track.compositor.is_none()
        && track.parent_compositor.is_none()
        && track.composite_depth == 0
        && !track.adjustment
        && track.motion.is_none()
        && track.parent_motion.is_none()
        && track.effects.is_empty()
        && track.envelopes.is_empty()
        && !track.invert_phase
        && track.surround.is_none()
        && track.ducking.is_none()
        && !track.duck_listener
        && track.output.is_master()
        && track.sends.is_empty()
}

fn plain_clip(clip: &Clip) -> bool {
    clip.level == 1.0
        && clip.rate == 1.0
        && clip.fade_in == Tick::ZERO
        && clip.fade_out == Tick::ZERO
        && clip.animation_in == Default::default()
        && clip.animation_out == Default::default()
        && clip.transition_fx.is_none()
        && clip.effects.is_empty()
        && clip.pan_crop.is_unset()
        && clip.mask.is_none()
        && clip.undersample == 1.0
        && clip.freeze.is_none()
        && clip.pitch == 0.0
        && !clip.invert_phase
        && clip.channels == Default::default()
        && clip.normalize.is_none()
        && !clip.auto_normalize
        && clip.audio_stream == 0
        && !clip.reduce_flicker
        && clip.resample.is_none()
        && clip.envelopes.is_empty()
        && clip.reverse.is_none()
        && clip.takes.is_empty()
}

fn aligned(rate: FrameRate, t: Tick) -> bool {
    rate.frame_start(rate.frame_at(t)) == t
}

fn request(project: &Project, settings: &RenderSettings, path: PathBuf) -> Option<Request> {
    let v = settings.video?;
    let a = settings.audio?;
    let range = settings.range;
    let p = &project.settings;
    if !settings.options.native_mp4
        || !matches!(settings.format, FileFormat::AvcMp4 | FileFormat::HevcMp4)
        || v.bit_depth != 8
        || v.frame_rate.den != 1
        || !(1..=120).contains(&v.frame_rate.num)
        || v.field_order != Default::default()
        || p.field_order != Default::default()
        || p.rotation != Default::default()
        || p.color_managed()
        || p.pixel_format.is_float()
        || p.pixel_format.is_video_levels()
        || p.output_360
        || !p.pixel_aspect.is_square()
        || p.channels.channel_count() != 2
        || a.channels != 2
        || a.sample_rate != 48_000
        || project.mute_all_video
        || !project.video_bus.is_default()
        || !project.video_output_fx.is_empty()
        || project.master != Default::default()
        || !project.master_fx.is_empty()
        || !project.busses.is_empty()
        || !project.fx_chains.is_empty()
        || project
            .tracks
            .iter()
            .any(|track| track.solo || track.composite_depth != 0)
        || v.width < 2
        || v.height < 2
        || v.width > 8192
        || v.height > 8192
        || !v.width.is_multiple_of(2)
        || !v.height.is_multiple_of(2)
        || range.start < Tick::ZERO
        || range.duration <= Tick::ZERO
        || !aligned(v.frame_rate, range.start)
        || !aligned(v.frame_rate, range.end())
    {
        return None;
    }

    let intersect =
        |clip: &&Clip| !clip.muted && clip.start < range.end() && clip.end() > range.start;
    let mut video = Vec::new();
    let mut audio = Vec::new();
    for track in project.tracks.iter().filter(|track| !track.muted) {
        for clip in track.clips.iter().filter(intersect) {
            if !plain_track(track) || !plain_clip(clip) {
                return None;
            }
            match track.kind {
                TrackKind::Video => video.push(clip),
                TrackKind::Audio => audio.push(clip),
            }
        }
    }
    video.sort_unstable_by_key(|clip| clip.start);
    let mut next = range.start;
    let mut clips = Vec::with_capacity(video.len());
    for clip in video {
        let start = clip.start.max(range.start);
        let end = clip.end().min(range.end());
        if start != next || !aligned(v.frame_rate, start) || !aligned(v.frame_rate, end) {
            return None;
        }
        let media = project.media(clip.media)?;
        let info = media.video?;
        let MediaSource::File { path } = &media.source else {
            return None;
        };
        if media.still
            || media.sequence.is_some()
            || media.layer.is_some()
            || !media.effects.is_empty()
            || media.color_space.is_some()
            || !info.pixel_aspect.is_square()
            || info.field_order != Default::default()
            || info.reading != Default::default()
            || info.spherical
            || info.full_range
            || info.frame_rate != v.frame_rate
            || u64::from(info.width) * u64::from(v.height)
                != u64::from(info.height) * u64::from(v.width)
            || u64::from(p.width) * u64::from(v.height) != u64::from(p.height) * u64::from(v.width)
        {
            return None;
        }
        let source_start = clip.source_in + (start - clip.start);
        let source_end = source_start + (end - start);
        if source_start < Tick::ZERO
            || source_end > media.duration
            || !aligned(info.frame_rate, source_start)
        {
            return None;
        }
        let paired = audio.iter().position(|other| {
            other.media == clip.media
                && other.start == clip.start
                && other.duration == clip.duration
                && other.source_in == clip.source_in
        });
        if let Some(i) = paired {
            if media.audio.is_none_or(|stream| stream.channels != 2)
                || !media.extra_audio.is_empty()
            {
                return None;
            }
            audio.swap_remove(i);
        }
        let source = std::path::absolute(path).ok()?;
        if !source.is_file() {
            return None;
        }
        clips.push(Segment {
            source,
            start: source_start.as_seconds_f64(),
            end: source_end.as_seconds_f64(),
            audio: paired.is_some(),
        });
        next = end;
    }
    if clips.is_empty() || next != range.end() || !audio.is_empty() {
        return None;
    }
    Some(Request {
        path,
        clips,
        width: v.width,
        height: v.height,
        fps: v.frame_rate.num,
        hevc: settings.format == FileFormat::HevcMp4,
        bitrate: v.bitrate,
        audio_bitrate: a.bitrate,
        frames: (v.frame_rate.frame_at(range.end()) - v.frame_rate.frame_at(range.start)) as u64,
    })
}

pub(super) fn try_export(
    project: &Project,
    settings: &RenderSettings,
    progress: &Mutex<RenderProgress>,
    cancel: &AtomicBool,
    repaint: &crate::RepaintFn,
    started: Instant,
) -> Option<Result<Vec<PathBuf>, String>> {
    if !settings.options.native_mp4 || cancel.load(Ordering::Relaxed) {
        return None;
    }
    let worker = std::env::current_exe()
        .ok()?
        .parent()?
        .join("native-export")
        .join("noni-export.exe");
    if !worker.is_file() {
        return None;
    }
    let output = std::path::absolute(&settings.path).ok()?;
    let mut job = request(project, settings, PathBuf::new())?;
    let folder = output.parent()?;
    std::fs::create_dir_all(folder).ok()?;
    let staging = tempfile::tempdir_in(folder).ok()?;
    job.path = staging.path().join("video.mp4");
    let input = staging.path().join("job.json");
    std::fs::write(&input, serde_json::to_vec(&job).ok()?).ok()?;
    let total = job.frames;
    let result = run_worker(&worker, &input, cancel, |done| {
        if let Ok(mut p) = progress.lock() {
            p.stage = "Rendering".into();
            p.encoder = "Reaction Station native exporter".into();
            p.done = done.min(total);
            p.total = total;
            p.elapsed_secs = started.elapsed().as_secs_f64();
            p.avg_fps = p.done as f64 / p.elapsed_secs.max(0.001);
            p.video = true;
        }
        repaint();
    });
    if cancel.load(Ordering::Relaxed) {
        return Some(super::cancelled_result());
    }
    if let Err(error) = result {
        eprintln!("Native MP4 export fell back to Noni: {error}");
        if let Ok(mut p) = progress.lock() {
            *p = RenderProgress {
                stage: "Preparing".into(),
                ..RenderProgress::default()
            };
        }
        repaint();
        return None;
    }
    if !job.path.is_file() {
        return None;
    }
    Some(
        std::fs::rename(&job.path, &output)
            .map(|()| vec![settings.path.clone()])
            .map_err(|e| format!("{}: {e}", output.display())),
    )
}

fn run_worker(
    worker: &Path,
    input: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64) + Send,
) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    let mut child = Command::new(worker)
        .arg(input)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take();
    std::thread::scope(|scope| {
        if let Some(stdout) = stdout {
            scope.spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if let Ok(done) = line.parse() {
                        progress(done);
                    }
                }
            });
        }
        loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                child.wait()?;
                return Ok(());
            }
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(status)) => return Err(std::io::Error::other(status.to_string())),
                Ok(None) => std::thread::sleep(Duration::from_millis(40)),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error);
                }
            }
        }
    })
}
