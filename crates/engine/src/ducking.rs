//! Auto Ducking's analysis in the background: the sound of each event of a
//! controller track read once, its channels mixed, for its loudness
//! (`bv_timeline::ducking::LevelMeter`) over the media time it plays and a
//! few seconds around it (so a trim keeps it).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bv_core::Tick;
use bv_timeline::ducking::LevelMeter;
use bv_timeline::{ClipId, DuckLevels, MediaInfo};

use crate::RepaintFn;

/// How far the analysis went, and what it found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DuckProgress {
    /// Seconds of sound read, of `total`.
    pub done: f64,
    pub total: f64,
    pub finished: bool,
    /// Each event read, and its loudness.
    pub results: Vec<(ClipId, DuckLevels)>,
    /// Events whose sound could not be read, and why.
    pub errors: Vec<(ClipId, String)>,
}

/// An event to read: its media and stream, and the media times around it.
#[derive(Clone, Debug)]
pub(crate) struct DuckSource {
    pub clip: ClipId,
    pub info: MediaInfo,
    pub stream: u16,
    pub from: Tick,
    pub to: Tick,
}

impl DuckSource {
    pub(crate) fn of(project: &bv_timeline::Project, clip: &bv_timeline::Clip) -> Option<Self> {
        let info = project.media(clip.media)?;
        info.audio_stream(clip.audio_stream)?;
        let (a, b) = bv_timeline::ducking::source_range(clip, info);
        let around = Tick::from_seconds_f64(AROUND);
        let from = (a.min(b) - around).max(Tick::ZERO);
        let to = (a.max(b) + around).min(info.duration);
        (to > from).then(|| Self {
            clip: clip.id,
            info: info.clone(),
            stream: clip.audio_stream,
            from,
            to,
        })
    }
}

/// Finish any missing simple-ducking analysis before exporting sound.
pub(crate) fn prepare_export(
    project: &mut Arc<bv_timeline::Project>,
    range: bv_core::TimeRange,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let sources: Vec<_> = project
        .pending_duck_clips()
        .filter(|c| c.start < range.end() && c.end() > range.start)
        .filter_map(|c| DuckSource::of(project, c))
        .collect();
    for source in sources {
        let levels = read(&source, &mut |_| !cancel.load(Ordering::Relaxed)).map_err(|error| {
            format!(
                "Could not read audio for ducking ({}): {error}",
                source.info.name
            )
        })?;
        if let Some(clip) = Arc::make_mut(project).clip_mut(source.clip) {
            clip.duck_levels = Some(levels);
        }
    }
    Ok(())
}

/// Media samples read at a time.
const BLOCK: usize = 1 << 16;
/// Media time read past each end of an event (seconds).
pub(crate) const AROUND: f64 = 5.0;

pub(crate) struct DuckJob {
    progress: Arc<Mutex<DuckProgress>>,
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl DuckJob {
    pub(crate) fn start(sources: Vec<DuckSource>, repaint: RepaintFn) -> DuckJob {
        let progress = Arc::new(Mutex::new(DuckProgress {
            total: sources
                .iter()
                .map(|s| (s.to - s.from).as_seconds_f64())
                .sum::<f64>()
                .max(1e-3),
            ..DuckProgress::default()
        }));
        let cancel = Arc::new(AtomicBool::new(false));
        let thread = {
            let progress = progress.clone();
            let cancel = cancel.clone();
            std::thread::Builder::new()
                .name("bv-auto-ducking".into())
                .spawn(move || {
                    let mut last = Instant::now();
                    for s in &sources {
                        let mut tick = |seconds: f64| {
                            progress.lock().expect("progress").done += seconds;
                            if last.elapsed() > Duration::from_millis(100) {
                                last = Instant::now();
                                repaint();
                            }
                            !cancel.load(Ordering::Relaxed)
                        };
                        let result = read(s, &mut tick);
                        if cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        let mut p = progress.lock().expect("progress");
                        match result {
                            Ok(levels) => p.results.push((s.clip, levels)),
                            Err(e) => p.errors.push((s.clip, e)),
                        }
                    }
                    let mut p = progress.lock().expect("progress");
                    p.finished = true;
                    p.done = p.total;
                    drop(p);
                    repaint();
                })
                .expect("could not start the auto ducking's thread")
        };
        DuckJob {
            progress,
            cancel,
            thread: Some(thread),
        }
    }

    pub(crate) fn progress(&self) -> DuckProgress {
        self.progress.lock().expect("progress").clone()
    }

    pub(crate) fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for DuckJob {
    fn drop(&mut self) {
        self.cancel();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// `s`'s loudness over its media times.
fn read(s: &DuckSource, tick: &mut dyn FnMut(f64) -> bool) -> Result<DuckLevels, String> {
    let mut reader = bv_media::open_audio_stream(&s.info, s.stream).map_err(|e| e.to_string())?;
    let rate = reader.sample_rate().0;
    let at = |t: Tick| (t.as_seconds_f64() * f64::from(rate)).round() as i64;
    let (a, b) = (at(s.from), at(s.to));
    let mut meter = LevelMeter::new(rate);
    let mut pos = a;
    while pos < b {
        let n = ((b - pos) as usize).min(BLOCK);
        let buf = reader.read(pos, n).map_err(|e| e.to_string())?;
        meter.push(&crate::audio_sync::mono(&buf));
        pos += n as i64;
        if !tick(n as f64 / f64::from(rate)) {
            return Err("canceled".into());
        }
    }
    let mut levels = meter.finish(s.from);
    levels.source = Some((s.info.id, s.stream, s.info.source.clone()));
    Ok(levels)
}
