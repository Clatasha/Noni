//! Optional per-export timings. Worker totals can overlap in wall time.

use std::io::{self, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::{RenderProgress, RenderSettings};
use crate::GpuStatus;

#[derive(Clone, Copy)]
pub(super) enum Stage {
    Draw,
    DecodeWait,
    SceneSetup,
    GpuCompose,
    GpuEffects,
    SourceUpload,
    SharedOutput,
    GpuWait,
    ReadbackCopy,
    VideoWrite,
    AudioMix,
    AudioWrite,
    QueueWait,
    Finish,
}

#[derive(Default)]
pub(super) struct Timing {
    nanos: [AtomicU64; Stage::Finish as usize + 1],
    draws: AtomicU64,
    gpu_draws: AtomicU64,
    readbacks: AtomicU64,
    readback_bytes: AtomicU64,
    shared_nv12_frames: AtomicU64,
    shared_nv12_status: std::sync::OnceLock<String>,
}

impl Timing {
    pub(super) fn add(&self, stage: Stage, duration: Duration) {
        self.nanos[stage as usize].fetch_add(
            duration.as_nanos().min(u128::from(u64::MAX)) as u64,
            Ordering::Relaxed,
        );
    }

    pub(super) fn measure<T>(timing: Option<&Self>, stage: Stage, f: impl FnOnce() -> T) -> T {
        let Some(timing) = timing else {
            return f();
        };
        let start = Instant::now();
        let result = f();
        timing.add(stage, start.elapsed());
        result
    }

    pub(super) fn draw(
        &self,
        elapsed: Duration,
        decode: Duration,
        scene_setup: Duration,
        gpu: bv_gpu::ReadbackTiming,
    ) {
        self.add(Stage::Draw, elapsed);
        self.add(Stage::DecodeWait, decode);
        self.add(Stage::SceneSetup, scene_setup);
        self.add(Stage::GpuCompose, gpu.compose);
        self.add(Stage::GpuEffects, gpu.effects);
        self.add(Stage::SourceUpload, gpu.source_upload);
        self.add(Stage::SharedOutput, gpu.shared_output);
        self.add(Stage::GpuWait, gpu.wait);
        self.add(Stage::ReadbackCopy, gpu.copy);
        self.draws.fetch_add(1, Ordering::Relaxed);
        if gpu.frames != 0 {
            self.gpu_draws.fetch_add(1, Ordering::Relaxed);
        }
        self.readbacks.fetch_add(gpu.frames, Ordering::Relaxed);
        self.readback_bytes.fetch_add(gpu.bytes, Ordering::Relaxed);
    }

    #[cfg(windows)]
    pub(super) fn shared_nv12(&self) {
        self.shared_nv12_frames.fetch_add(1, Ordering::Relaxed);
    }

    #[cfg(windows)]
    pub(super) fn set_shared_nv12_status(&self, status: String) {
        let _ = self.shared_nv12_status.set(status);
    }

    fn seconds(&self, stage: Stage) -> f64 {
        Duration::from_nanos(self.nanos[stage as usize].load(Ordering::Relaxed)).as_secs_f64()
    }

    pub(super) fn report(
        &self,
        settings: &RenderSettings,
        progress: &RenderProgress,
        compositor: &GpuStatus,
        decoding: &bv_mf::HardwareDecoding,
    ) -> io::Result<()> {
        let mut path = settings.path.as_os_str().to_owned();
        path.push(".timing.txt");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        let status = if progress.cancelled {
            "cancelled"
        } else if progress.error.is_some() {
            "failed"
        } else {
            "completed"
        };
        let draw = self.seconds(Stage::Draw);
        let decode = self.seconds(Stage::DecodeWait);
        let gpu = self.seconds(Stage::GpuWait);
        let copy = self.seconds(Stage::ReadbackCopy);
        writeln!(file, "Noni export timing")?;
        writeln!(file, "Status: {status}")?;
        writeln!(file, "Wall time: {:.3} s", progress.elapsed_secs)?;
        writeln!(
            file,
            "Timeline range: {:.3} s",
            settings.range.duration.as_seconds_f64()
        )?;
        if let Some(video) = settings.video {
            writeln!(
                file,
                "Requested video: {}x{}, {:.3} fps, {} kbps, {:?}",
                video.width,
                video.height,
                video.frame_rate.as_f64(),
                video.bitrate / 1000,
                video.encoder
            )?;
        }
        writeln!(file, "Output format: {:?}", settings.format)?;
        let backend = if progress.reaction_station {
            "Reaction Station"
        } else {
            "Noni"
        };
        writeln!(file, "Export engine: {backend}")?;
        if let Some(reason) = &progress.native_fallback {
            writeln!(file, "Reaction Station fallback: {reason}")?;
        }
        if progress.reaction_station {
            let encoder = if progress.encoder.is_empty() {
                "not reported"
            } else {
                &progress.encoder
            };
            writeln!(file, "Video encoder: {encoder}")?;
            writeln!(file, "Output units: {} / {}", progress.done, progress.total)?;
            return file.flush();
        }
        writeln!(file, "Video encoder: {}", progress.encoder)?;
        writeln!(file, "Compositor status at end: {compositor:?}")?;
        writeln!(
            file,
            "Hardware decoding preference: {decoding:?} (can fall back per source)"
        )?;
        if let Some(status) = self.shared_nv12_status.get() {
            writeln!(file, "Fast NVENC: {status}")?;
            writeln!(
                file,
                "Video frames handed off on the GPU: {}",
                self.shared_nv12_frames.load(Ordering::Relaxed)
            )?;
        }
        writeln!(file, "Output units: {} / {}", progress.done, progress.total)?;
        writeln!(
            file,
            "Draw calls with GPU readback: {} / {}",
            self.gpu_draws.load(Ordering::Relaxed),
            self.draws.load(Ordering::Relaxed)
        )?;
        writeln!(
            file,
            "GPU readbacks: {} ({:.1} MiB)",
            self.readbacks.load(Ordering::Relaxed),
            self.readback_bytes.load(Ordering::Relaxed) as f64 / 1048576.0
        )?;
        writeln!(file)?;
        writeln!(file, "Cumulative stage time (worker threads can overlap)")?;
        writeln!(file, "Video drawing, total: {draw:.3} s")?;
        let setup = self.seconds(Stage::SceneSetup);
        let compose = self.seconds(Stage::GpuCompose);
        let shared = self.seconds(Stage::SharedOutput);
        writeln!(file, "  GPU scene setup: {setup:.3} s")?;
        writeln!(file, "  GPU composition (host time): {compose:.3} s")?;
        writeln!(
            file,
            "    Decoded YUV upload staging (host time): {:.3} s",
            self.seconds(Stage::SourceUpload)
        )?;
        writeln!(
            file,
            "    Effect preparation and synchronous work: {:.3} s",
            self.seconds(Stage::GpuEffects)
        )?;
        writeln!(
            file,
            "  Shared NV12 submission and synchronization: {shared:.3} s"
        )?;
        writeln!(
            file,
            "  Source video reader wait (overlaps drawing): {decode:.3} s"
        )?;
        writeln!(
            file,
            "  GPU submission to mapped output (execution and transfer): {gpu:.3} s"
        )?;
        writeln!(file, "  Mapped output copy and unpack: {copy:.3} s")?;
        writeln!(
            file,
            "  Other drawing, setup and CPU work: {:.3} s",
            (draw - setup - compose - shared - gpu - copy).max(0.0)
        )?;
        for (label, stage) in [
            ("Video encoding and writing", Stage::VideoWrite),
            ("Audio mixing", Stage::AudioMix),
            ("Audio encoding and writing", Stage::AudioWrite),
            ("Waiting to hand work to the writer", Stage::QueueWait),
            ("Encoder drain and file finalization", Stage::Finish),
        ] {
            writeln!(file, "{label}: {:.3} s", self.seconds(stage))?;
        }
        file.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{FileFormat, RenderOptions, VideoParams};
    use bv_core::{FrameRate, Tick, TimeRange};
    use std::sync::{Arc, Mutex, atomic::AtomicBool};

    #[test]
    fn rendering_collects_timings_and_keeps_an_existing_report() {
        let dir = std::env::temp_dir().join(format!("noni-timing-{}", std::process::id()));
        std::fs::create_dir(&dir).expect("report directory");
        let mut project = bv_timeline::Project::default();
        project.settings.width = 32;
        project.settings.height = 18;
        project.settings.frame_rate = FrameRate::FPS_30;
        let track = project.tracks[0].id;
        let media = project.add_media(bv_timeline::MediaInfo::generated(
            bv_timeline::Generator::ColorBars,
            &project.settings,
        ));
        project
            .insert_media(media, Tick::ZERO, Some(track), None)
            .expect("clip");
        let settings = RenderSettings {
            path: dir.join("frame.png"),
            format: FileFormat::Png,
            range: TimeRange::new(Tick::ZERO, FrameRate::FPS_30.frame_start(2)),
            video: Some(VideoParams {
                width: 32,
                height: 18,
                frame_rate: FrameRate::FPS_30,
                ..VideoParams::default()
            }),
            audio: None,
            options: RenderOptions::default(),
            project_file: None,
        };
        let repaint: crate::RepaintFn = Arc::new(|| {});
        let accel = crate::accel::Accel::new(repaint.clone());
        let mut painter = crate::accel::Painter::new(accel, crate::plugins::Purpose::File);
        painter.measure_readback();
        let timing = Arc::new(Timing::default());
        let progress = Mutex::new(RenderProgress::default());
        let files = super::super::run(
            &project,
            &settings,
            &progress,
            &AtomicBool::new(false),
            &super::super::Shown::default(),
            &repaint,
            Instant::now(),
            &mut painter,
            Some(&timing),
        )
        .expect("render");
        assert_eq!(files.len(), 2);
        assert_eq!(timing.draws.load(Ordering::Relaxed), 2);
        assert!(timing.seconds(Stage::Draw) > 0.0);
        assert!(timing.seconds(Stage::VideoWrite) > 0.0);
        let progress = progress.into_inner().expect("progress");
        let decoding = bv_mf::HardwareDecoding::Off;
        let path = dir.join("frame.png.timing.txt");
        timing
            .report(&settings, &progress, &GpuStatus::Off, &decoding)
            .expect("report");
        let report = std::fs::read_to_string(&path).expect("read report");
        assert!(report.contains("Status: completed"));
        assert!(report.contains("GPU scene setup:"));
        assert!(report.contains("Effect preparation and synchronous work:"));
        assert!(report.contains("Shared NV12 submission and synchronization:"));
        assert!(report.contains("Output units: 2 / 2"));
        let error = timing
            .report(&settings, &progress, &GpuStatus::Off, &decoding)
            .expect_err("existing report");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            std::fs::read_to_string(path).expect("read report again"),
            report
        );
        std::fs::remove_dir_all(dir).expect("cleanup");
    }
}
