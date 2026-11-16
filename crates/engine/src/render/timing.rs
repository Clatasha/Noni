//! Optional per-export timings. Worker totals can overlap in wall time.

use std::io::{self, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::{RenderProgress, RenderSettings};
use crate::GpuStatus;

#[derive(Clone, Copy)]
pub(super) enum Stage {
    Draw,
    SourcePrepare,
    SourceRead,
    DecodeWait,
    SceneSetup,
    GpuCompose,
    GpuEffects,
    SourceUpload,
    SharedOutput,
    GpuWait,
    ReadbackCopy,
    VideoWrite,
    #[cfg(windows)]
    SharedPrepare,
    #[cfg(windows)]
    SharedEncode,
    #[cfg(windows)]
    SharedEventWait,
    #[cfg(windows)]
    SharedCpuWait,
    #[cfg(windows)]
    SharedMux,
    DuckingAnalysis,
    AudioMix,
    AudioWrite,
    QueueWait,
    Finish,
}

#[derive(Default)]
pub(crate) struct Timing {
    nanos: [AtomicU64; Stage::Finish as usize + 1],
    draws: AtomicU64,
    source_uploads: AtomicU64,
    belt_uploads: AtomicU64,
    direct_overlay_blends: AtomicU64,
    titles: OverlayTotals,
    still_images: OverlayTotals,
    painters: AtomicU64,
    prepared_frames: AtomicU64,
    prepared_pictures: AtomicU64,
    reused_pictures: AtomicU64,
    gpu_draws: AtomicU64,
    readbacks: AtomicU64,
    readback_bytes: AtomicU64,
    shared_nv12_frames: AtomicU64,
    shared_surfaces_reused: AtomicU64,
    shared_surfaces_created: AtomicU64,
    #[cfg(windows)]
    shared_nv12_pooled: AtomicU64,
    shared_nv12_status: std::sync::OnceLock<String>,
}

#[derive(Default)]
struct OverlayTotals {
    read: AtomicU64,
    read_count: AtomicU64,
    upload: AtomicU64,
    upload_bytes: AtomicU64,
    place: AtomicU64,
    place_pixels: AtomicU64,
    blend: AtomicU64,
    blend_pixels: AtomicU64,
}

impl OverlayTotals {
    fn add(&self, timing: bv_gpu::OverlayTiming) {
        for (counter, value) in [
            (&self.read, timing.read.as_nanos() as u64),
            (&self.read_count, timing.read_count),
            (&self.upload, timing.upload.as_nanos() as u64),
            (&self.upload_bytes, timing.upload_bytes),
            (&self.place, timing.place.as_nanos() as u64),
            (&self.place_pixels, timing.place_pixels),
            (&self.blend, timing.blend.as_nanos() as u64),
            (&self.blend_pixels, timing.blend_pixels),
        ] {
            if value != 0 {
                counter.fetch_add(value, Ordering::Relaxed);
            }
        }
    }

    fn report(&self, file: &mut impl Write, label: &str) -> io::Result<()> {
        let count = self.read_count.load(Ordering::Relaxed);
        let seconds = |value: &AtomicU64| value.load(Ordering::Relaxed) as f64 / 1e9;
        let megapixels = |value: &AtomicU64| value.load(Ordering::Relaxed) as f64 / 1e6;
        writeln!(
            file,
            "    {label} source reads (including cached): {:.3} s / {count}",
            seconds(&self.read)
        )?;
        writeln!(
            file,
            "    {label} RGBA uploads (host): {:.3} s / {:.1} MiB",
            seconds(&self.upload),
            self.upload_bytes.load(Ordering::Relaxed) as f64 / 1048576.0
        )?;
        writeln!(
            file,
            "    {label} placement commands (host): {:.3} s / {:.1} MP",
            seconds(&self.place),
            megapixels(&self.place_pixels)
        )?;
        writeln!(
            file,
            "    {label} track blend commands (host): {:.3} s / {:.1} MP",
            seconds(&self.blend),
            megapixels(&self.blend_pixels)
        )
    }
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

    pub(super) fn set_painters(&self, count: usize) {
        self.painters.store(count as u64, Ordering::Relaxed);
    }

    pub(super) fn reused_sources(&self, count: u64) {
        self.reused_pictures.fetch_add(count, Ordering::Relaxed);
    }

    pub(super) fn prepared_sources(&self, elapsed: Duration, pictures: usize) {
        self.add(Stage::SourcePrepare, elapsed);
        self.prepared_frames.fetch_add(1, Ordering::Relaxed);
        self.prepared_pictures
            .fetch_add(pictures as u64, Ordering::Relaxed);
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
        self.source_uploads
            .fetch_add(gpu.source_uploads, Ordering::Relaxed);
        self.belt_uploads
            .fetch_add(gpu.belt_uploads, Ordering::Relaxed);
        self.direct_overlay_blends
            .fetch_add(gpu.direct_overlay_blends, Ordering::Relaxed);
        self.titles.add(gpu.titles);
        self.still_images.add(gpu.still_images);
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
    pub(super) fn shared_surface(&self, reused: bool) {
        let counter = if reused {
            &self.shared_surfaces_reused
        } else {
            &self.shared_surfaces_created
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    #[cfg(windows)]
    pub(super) fn shared_nv12(&self) {
        self.shared_nv12_frames.fetch_add(1, Ordering::Relaxed);
    }

    #[cfg(windows)]
    pub(super) fn record_shared_writer(&self, timing: bv_mf::SharedNv12Timing) {
        self.add(Stage::SharedPrepare, timing.prepare);
        self.add(Stage::SharedEncode, timing.encode);
        self.add(Stage::SharedEventWait, timing.event_wait);
        if timing.pooled {
            self.shared_nv12_pooled.fetch_add(1, Ordering::Relaxed);
        }
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
            "Noni Native MP4"
        } else {
            "Noni"
        };
        writeln!(file, "Export engine: {backend}")?;
        if let Some(reason) = &progress.native_fallback {
            writeln!(file, "Native MP4 fallback: {reason}")?;
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
        writeln!(
            file,
            "Shared GPU output surfaces: {} reused, {} newly allocated",
            self.shared_surfaces_reused.load(Ordering::Relaxed),
            self.shared_surfaces_created.load(Ordering::Relaxed)
        )?;
        writeln!(
            file,
            "Reusable decoded upload staging: {} / {} uploads",
            self.belt_uploads.load(Ordering::Relaxed),
            self.source_uploads.load(Ordering::Relaxed)
        )?;
        let painters = self.painters.load(Ordering::Relaxed);
        writeln!(file, "Video drawing workers: {painters}")?;
        let prepared = self.prepared_frames.load(Ordering::Relaxed);
        if prepared != 0 {
            writeln!(
                file,
                "Shared source decoding: enabled (ordered producer, bounded frame queues)"
            )?;
            writeln!(file, "Frames prepared by shared decoder: {prepared}")?;
            writeln!(
                file,
                "Source pictures prepared: {}",
                self.prepared_pictures.load(Ordering::Relaxed)
            )?;
            writeln!(
                file,
                "Prepared source picture requests reused: {}",
                self.reused_pictures.load(Ordering::Relaxed)
            )?;
        }
        if prepared == 0 {
            let mode = if painters <= 1 {
                "single drawing worker"
            } else {
                "per-worker readers"
            };
            writeln!(file, "Source decoding mode: {mode}")?;
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
            "    Single-pass overlay blends: {}",
            self.direct_overlay_blends.load(Ordering::Relaxed)
        )?;
        self.titles.report(&mut file, "Title and credit roll")?;
        self.still_images.report(&mut file, "Still image")?;
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
            (
                "Shared source preparation (overlaps drawing)",
                Stage::SourcePrepare,
            ),
            (
                "  Shared video reader fetch and decode (within source preparation)",
                Stage::SourceRead,
            ),
            ("Video encoding and writing", Stage::VideoWrite),
            ("Ducking analysis before rendering", Stage::DuckingAnalysis),
            ("Audio mixing", Stage::AudioMix),
            ("Audio encoding and writing", Stage::AudioWrite),
            ("Waiting to hand work to the writer", Stage::QueueWait),
            ("Encoder drain and file finalization", Stage::Finish),
        ] {
            writeln!(file, "{label}: {:.3} s", self.seconds(stage))?;
        }
        #[cfg(windows)]
        {
            let shared_frames = self.shared_nv12_frames.load(Ordering::Relaxed);
            if shared_frames != 0 {
                let pooled = self.shared_nv12_pooled.load(Ordering::Relaxed);
                writeln!(file, "Fast NVENC writer breakdown ({shared_frames} frames)")?;
                writeln!(
                    file,
                    "  Pooled encoder surfaces: {pooled} / {shared_frames}"
                )?;
                for (label, stage) in [
                    ("Open, copy and flush encoder texture", Stage::SharedPrepare),
                    ("CPU wait for compositor output", Stage::SharedCpuWait),
                    ("Wait for encoder events", Stage::SharedEventWait),
                    ("Submit frames and retrieve packets", Stage::SharedEncode),
                    ("Write encoded video to MP4", Stage::SharedMux),
                ] {
                    writeln!(file, "  {label}: {:.3} s", self.seconds(stage))?;
                }
            }
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
        assert!(report.contains("Ducking analysis before rendering: 0.000 s"));
        assert!(report.contains("Title and credit roll source reads (including cached):"));
        assert!(report.contains("Still image RGBA uploads (host):"));
        assert!(
            report.contains("Shared video reader fetch and decode (within source preparation):")
        );
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
