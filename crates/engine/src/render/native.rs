//! Runs the packaged native export worker for timelines it can reproduce.

use std::collections::BTreeSet;
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
        && !track.has_automation()
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
    if v.rate_mode == super::RateMode::Cbr {
        return Err("CBR output uses the Noni encoder.");
    }
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
            "The native MP4 exporter requires progressive 8-bit MP4 at even dimensions up to 8192.",
        );
    }
    if v.frame_rate.den != 1 || !(1..=120).contains(&v.frame_rate.num) {
        return Err(
            "The native MP4 exporter requires a whole-number output frame rate from 1 to 120 fps.",
        );
    }
    if p.channels.channel_count() != 2 || a.channels != 2 || a.sample_rate != 48_000 {
        return Err("The native MP4 exporter requires a stereo project and 48 kHz stereo output.");
    }
    if p.field_order != Default::default()
        || p.rotation != Default::default()
        || p.color_managed()
        || p.pixel_format.is_float()
        || p.pixel_format.is_video_levels()
        || p.output_360
        || !p.pixel_aspect.is_square()
    {
        return Err("The project uses video settings the native MP4 exporter cannot reproduce.");
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
        // The worker mixes all embedded audio streams. Only hand it audio
        // when the timeline uses each stream once with the same source trim.
        let mut streams = BTreeSet::new();
        let mut valid_streams = true;
        audio.retain(|other| {
            let paired = other.media == clip.media
                && other.start.max(range.start) == start
                && other.end().min(range.end()) == end
                && other.source_in + (start - other.start) == source_start;
            if paired {
                valid_streams &= streams.insert(other.audio_stream)
                    && media
                        .audio_stream(other.audio_stream)
                        .is_some_and(|stream| stream.channels == 2);
            }
            !paired
        });
        let has_audio = !streams.is_empty();
        if has_audio && (!valid_streams || streams.len() != media.audio_stream_count()) {
            return Err("Each embedded audio stream must be stereo and used exactly once.");
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
            audio: has_audio,
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
        .map_err(|error| format!("Could not locate the native MP4 export worker: {error}"))?;
    let worker = exe.with_file_name("native-export").join("noni-export.exe");
    if !worker.is_file() {
        return Err("The native MP4 export worker is missing from this installation.".into());
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
    let log = staging.path().join("worker.log");
    let result = run_worker(&worker, &input, &log, cancel, |done| {
        let mut p = progress.lock().expect("progress");
        p.stage = "Rendering".into();
        p.done = done.min(total);
        p.elapsed_secs = started.elapsed().as_secs_f64();
        p.avg_fps = p.done as f64 / p.elapsed_secs.max(0.001);
        drop(p);
        repaint();
    });
    if cancel.load(Ordering::Relaxed) {
        return Ok(super::cancelled_result());
    }
    result.map_err(|error| format!("Native MP4 export worker failed: {error}"))?;
    if !job.path.is_file() {
        return Err("The native MP4 export worker finished without producing an MP4.".into());
    }
    let bytes = job.path.metadata().map_or(0, |metadata| metadata.len());
    let result = std::fs::rename(&job.path, &output)
        .map(|()| vec![settings.path.clone()])
        .map_err(|e| format!("{}: {e}", output.display()));
    let encoder = worker_encoder(&log).unwrap_or_default();
    let mut p = progress.lock().expect("progress");
    p.bytes = bytes;
    p.encoder = encoder;
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

fn worker_encoder(log: &Path) -> Option<String> {
    // Matches the pinned worker's encoder-selection diagnostic after open succeeds.
    let log = BufReader::new(std::fs::File::open(log).ok()?);
    for line in log.split(b'\n').map_while(Result::ok) {
        let line = String::from_utf8_lossy(&line);
        if let Some((name, _)) = line
            .strip_prefix("[pipeline] encodeur vidéo : ")
            .and_then(|value| value.split_once(" ("))
        {
            return Some(name.to_owned());
        }
    }
    None
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{AudioParams, RenderOptions, VideoParams};
    use bv_core::{SampleRate, TimeRange};
    use bv_timeline::{AudioStreamInfo, ClipId, Generator, MediaInfo, ProjectSettings};

    #[test]
    fn embedded_streams_must_match_the_timeline_mix() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("recording.mp4");
        std::fs::File::create(&source).unwrap();
        let mut project = Project::new(ProjectSettings {
            frame_rate: FrameRate::FPS_60,
            ..ProjectSettings::default()
        });
        let mut media = MediaInfo::generated(Generator::ColorBars, &project.settings);
        media.source = MediaSource::File {
            path: source.to_str().unwrap().to_owned(),
        };
        let stereo = AudioStreamInfo {
            sample_rate: SampleRate(96_000),
            channels: 2,
        };
        media.audio = Some(stereo);
        media.extra_audio.push(stereo);
        let media = project.add_media(media);
        let second_id = project.add_track(TrackKind::Audio);
        for (index, track) in project.tracks.iter_mut().enumerate() {
            let mut clip = Clip::new(ClipId(10 + index as u64), media, Tick::ZERO, Tick::SECOND);
            clip.audio_stream = u16::from(track.id == second_id);
            track.clips.push(clip);
        }
        let path = dir.path().join("export.mp4");
        let settings = RenderSettings {
            format: FileFormat::AvcMp4,
            path: path.clone(),
            range: TimeRange::new(Tick::ZERO, Tick::SECOND),
            video: Some(VideoParams {
                frame_rate: FrameRate::FPS_60,
                ..VideoParams::default()
            }),
            audio: Some(AudioParams::default()),
            options: RenderOptions::default(),
            project_file: None,
        };
        let job = request(&project, &settings, path.clone()).expect("both streams are aligned");
        assert_eq!(job.clips.len(), 1);
        assert!(job.clips[0].audio);
        let mut cbr = settings.clone();
        cbr.video.as_mut().unwrap().rate_mode = super::super::RateMode::Cbr;
        assert_eq!(
            request(&project, &cbr, path.clone()).err().unwrap(),
            "CBR output uses the Noni encoder."
        );

        let second = project
            .tracks
            .iter_mut()
            .find(|track| track.id == second_id)
            .unwrap();
        second.muted = true;
        assert!(request(&project, &settings, path.clone()).is_err());

        let second = project
            .tracks
            .iter_mut()
            .find(|track| track.id == second_id)
            .unwrap();
        second.muted = false;
        second.clips[0].audio_stream = 0;
        assert!(request(&project, &settings, path).is_err());
    }
}
