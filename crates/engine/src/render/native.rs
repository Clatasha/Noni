//! Runs the packaged native export worker for timelines it can reproduce.

use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
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

fn request(
    project: &Project,
    settings: &RenderSettings,
    path: PathBuf,
) -> Result<Request, &'static str> {
    let v = settings.video.ok_or("Video output is disabled.")?;
    let a = settings.audio.ok_or("Audio output is disabled.")?;
    let range = settings.range;
    let p = &project.settings;
    if !matches!(settings.format, FileFormat::AvcMp4 | FileFormat::HevcMp4)
        || v.bit_depth != 8
        || v.field_order != Default::default()
        || v.width < 2
        || v.height < 2
        || v.width > 8192
        || v.height > 8192
        || !v.width.is_multiple_of(2)
        || !v.height.is_multiple_of(2)
    {
        return Err(
            "Reaction Station requires progressive 8-bit MP4 at even dimensions up to 8192.",
        );
    }
    if v.frame_rate.den != 1 || !(1..=120).contains(&v.frame_rate.num) {
        return Err(
            "Reaction Station requires a whole-number output frame rate from 1 to 120 fps.",
        );
    }
    if p.channels.channel_count() != 2 || a.channels != 2 || a.sample_rate != 48_000 {
        return Err("Reaction Station requires a stereo project and 48 kHz stereo output.");
    }
    if p.field_order != Default::default()
        || p.rotation != Default::default()
        || p.color_managed()
        || p.pixel_format.is_float()
        || p.pixel_format.is_video_levels()
        || p.output_360
        || !p.pixel_aspect.is_square()
    {
        return Err("The project uses video settings Reaction Station cannot reproduce.");
    }
    if project.mute_all_video
        || !project.video_bus.is_default()
        || !project.video_output_fx.is_empty()
    {
        return Err("The project uses video output effects, routing, or a global video mute.");
    }
    if project.master != Default::default()
        || !project.master_fx.is_empty()
        || !project.busses.is_empty()
        || !project.fx_chains.is_empty()
    {
        return Err("The project uses master effects or custom mixer settings.");
    }
    if project
        .tracks
        .iter()
        .any(|track| track.solo || track.composite_depth != 0)
    {
        return Err("The project uses soloed tracks or nested compositing.");
    }
    if range.start < Tick::ZERO
        || range.duration <= Tick::ZERO
        || !aligned(v.frame_rate, range.start)
        || !aligned(v.frame_rate, range.end())
    {
        return Err("The export range does not start and end on output frame boundaries.");
    }

    let intersect =
        |clip: &&Clip| !clip.muted && clip.start < range.end() && clip.end() > range.start;
    let mut video = Vec::new();
    let mut audio = Vec::new();
    for track in project.tracks.iter().filter(|track| !track.muted) {
        for clip in track.clips.iter().filter(intersect) {
            if !plain_track(track) {
                return Err("An active track uses effects, motion, mixing, or routing controls.");
            }
            if !plain_clip(clip) {
                return Err(
                    "An active clip uses effects, fades, transforms, or other clip controls.",
                );
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
        if start != next {
            return Err("The export range contains a video gap or overlapping video clips.");
        }
        if !aligned(v.frame_rate, start) || !aligned(v.frame_rate, end) {
            return Err("A video cut falls between output frames.");
        }
        let media = project
            .media(clip.media)
            .ok_or("A video clip has missing media.")?;
        let info = media.video.ok_or("A video clip has no video stream.")?;
        let MediaSource::File { path } = &media.source else {
            return Err("The timeline contains generated or nested video.");
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
        {
            return Err(
                "A video source uses a still image, sequence, or unsupported media settings.",
            );
        }
        if info.frame_rate != v.frame_rate {
            return Err("A source video frame rate differs from the output frame rate.");
        }
        if u64::from(info.width) * u64::from(v.height)
            != u64::from(info.height) * u64::from(v.width)
            || u64::from(p.width) * u64::from(v.height) != u64::from(p.height) * u64::from(v.width)
        {
            return Err("The source, project, and output aspect ratios must match.");
        }
        let source_start = clip.source_in + (start - clip.start);
        let source_end = source_start + (end - start);
        if source_start < Tick::ZERO
            || source_end > media.duration
            || !aligned(info.frame_rate, source_start)
        {
            return Err("A source trim falls outside the media or starts between source frames.");
        }
        // Only the portion inside the export range has to match. An audio
        // clip may extend beyond the loop selection without affecting it.
        let paired = audio.iter().position(|other| {
            other.media == clip.media
                && other.start.max(range.start) == start
                && other.end().min(range.end()) == end
                && other.source_in + (start - other.start) == source_start
        });
        if let Some(i) = paired {
            if media.audio.is_none_or(|stream| stream.channels != 2)
                || !media.extra_audio.is_empty()
            {
                return Err("A paired source must have exactly one stereo audio stream.");
            }
            audio.swap_remove(i);
        }
        let source =
            std::path::absolute(path).map_err(|_| "A source path could not be resolved.")?;
        if !source.is_file() {
            return Err("A source video file is missing.");
        }
        clips.push(Segment {
            source,
            start: source_start.as_seconds_f64(),
            end: source_end.as_seconds_f64(),
            audio: paired.is_some(),
        });
        next = end;
    }
    if clips.is_empty() || next != range.end() {
        return Err("Video does not cover the entire export range.");
    }
    if !audio.is_empty() {
        return Err("The timeline has audio that is separate from its matching video.");
    }
    Ok(Request {
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
) -> Result<Result<Vec<PathBuf>, String>, String> {
    if cancel.load(Ordering::Relaxed) {
        return Ok(super::cancelled_result());
    }
    let mut job = request(project, settings, PathBuf::new())?;
    let exe = std::env::current_exe()
        .map_err(|error| format!("Could not locate the Reaction Station worker: {error}"))?;
    let worker = exe.with_file_name("native-export").join("noni-export.exe");
    if !worker.is_file() {
        return Err("The Reaction Station worker is missing from this installation.".into());
    }
    let output = std::path::absolute(&settings.path)
        .map_err(|error| format!("Could not resolve the output path: {error}"))?;
    let folder = output
        .parent()
        .ok_or("The output path has no parent folder.")?;
    std::fs::create_dir_all(folder)
        .map_err(|error| format!("Could not create the output folder: {error}"))?;
    let staging = tempfile::tempdir_in(folder)
        .map_err(|error| format!("Could not prepare the native export folder: {error}"))?;
    job.path = staging.path().join("video.mp4");
    let input = staging.path().join("job.json");
    let request = serde_json::to_vec(&job)
        .map_err(|error| format!("Could not encode the native export request: {error}"))?;
    std::fs::write(&input, request)
        .map_err(|error| format!("Could not write the native export request: {error}"))?;
    let total = job.frames;
    {
        let mut p = progress.lock().expect("progress");
        p.reaction_station = true;
        p.total = total;
        p.video = true;
    }
    repaint();
    let result = run_worker(
        &worker,
        &input,
        &staging.path().join("worker.log"),
        cancel,
        |done| {
            let mut p = progress.lock().expect("progress");
            p.stage = "Rendering".into();
            p.done = done.min(total);
            p.elapsed_secs = started.elapsed().as_secs_f64();
            p.avg_fps = p.done as f64 / p.elapsed_secs.max(0.001);
            drop(p);
            repaint();
        },
    );
    if cancel.load(Ordering::Relaxed) {
        return Ok(super::cancelled_result());
    }
    result.map_err(|error| format!("Reaction Station worker failed: {error}"))?;
    if !job.path.is_file() {
        return Err("The Reaction Station worker finished without producing an MP4.".into());
    }
    let bytes = job.path.metadata().map_or(0, |metadata| metadata.len());
    let result = std::fs::rename(&job.path, &output)
        .map(|()| vec![settings.path.clone()])
        .map_err(|e| format!("{}: {e}", output.display()));
    let mut p = progress.lock().expect("progress");
    p.bytes = bytes;
    Ok(result)
}

fn run_worker(
    worker: &Path,
    input: &Path,
    log: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64) + Send,
) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    let mut child = Command::new(worker)
        .arg(input)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(std::fs::File::create(log)?)
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
                Ok(Some(status)) => {
                    let detail = worker_error(log).unwrap_or_else(|| status.to_string());
                    return Err(std::io::Error::other(detail));
                }
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

fn worker_error(log: &Path) -> Option<String> {
    let mut file = std::fs::File::open(log).ok()?;
    let offset = file.metadata().ok()?.len().saturating_sub(4096);
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut tail = Vec::new();
    file.take(4096).read_to_end(&mut tail).ok()?;
    let tail = String::from_utf8_lossy(&tail);
    let detail = tail
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix("Error: "))
        .or_else(|| tail.lines().rev().find(|line| !line.trim().is_empty()))?;
    Some(detail.chars().take(320).collect())
}
