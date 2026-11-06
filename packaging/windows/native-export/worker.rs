//! Standalone entry point to the pinned Reaction Station compositor crate.

use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use openscreen_compositor::{
    compositor::{Compositor, live_params_from_scene},
    config::Cfg,
    d3d::Gpu,
    export_control::ExportControl,
    pipeline::{self, ClipSource, ExportCodec, ExportParams},
    scene::Scene,
};
use serde::Deserialize;

#[derive(Deserialize)]
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

#[derive(Deserialize)]
struct Segment {
    source: PathBuf,
    start: f64,
    end: f64,
    audio: bool,
}

fn main() -> Result<()> {
    let arg = std::env::args_os()
        .nth(1)
        .context("expected a render job file")?;
    if arg == "--version" {
        println!("Noni native export worker 1");
        return Ok(());
    }
    if arg == "--check-scene" {
        full_frame_scene(&[], &ExportParams::default()).context("invalid export scene")?;
        return Ok(());
    }
    let request: Request = serde_json::from_slice(&std::fs::read(arg)?)?;
    let clips = request
        .clips
        .into_iter()
        .map(|clip| {
            Ok(ClipSource {
                screen: clip
                    .source
                    .into_os_string()
                    .into_string()
                    .map_err(|_| anyhow::anyhow!("source path is not Unicode"))?,
                webcam: String::new(),
                source_start_sec: clip.start,
                source_end_sec: clip.end,
                webcam_offset_sec: 0.0,
                has_audio: clip.audio,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let params = ExportParams {
        width: request.width,
        height: request.height,
        fps: Some(request.fps),
        codec: if request.hevc {
            ExportCodec::H265
        } else {
            ExportCodec::H264
        },
        bit_rate: Some(i64::from(request.bitrate)),
        audio_bit_rate: Some(i64::from(request.audio_bitrate)),
    };
    let gpu = Gpu::create(false)?;
    let compositor = Compositor::new_sized(&gpu, params.width, params.height)?;
    // Use the application's real scene path. Without a scene the upstream
    // compositor uses its benchmark fixture's geometry.
    let scene = full_frame_scene(&clips, &params)?;
    compositor.set_live_params(live_params_from_scene(&scene));
    compositor.set_scene(Some(scene));
    let cfg = Cfg::by_name("C0").context("primary video configuration is unavailable")?;
    let path = request
        .path
        .to_str()
        .context("output path is not Unicode")?;
    let stdout = io::stdout();
    let mut last = None;
    let stats = pipeline::run_composited_multi_cancellable(
        &clips,
        path,
        &gpu,
        &compositor,
        &cfg,
        &params,
        &mut |frames| {
            if last.is_none_or(|at: Instant| at.elapsed() >= Duration::from_millis(100)) {
                last = Some(Instant::now());
                let mut out = stdout.lock();
                let _ = writeln!(out, "{frames}").and_then(|()| out.flush());
            }
        },
        &ExportControl::default(),
    )?;
    anyhow::ensure!(
        stats.frames == request.frames,
        "native exporter wrote {} of {} frames",
        stats.frames,
        request.frames
    );
    println!("{}", stats.frames);
    Ok(())
}

fn full_frame_scene(clips: &[ClipSource], params: &ExportParams) -> Result<Scene> {
    let clips: Vec<_> = clips
        .iter()
        .map(|clip| {
            serde_json::json!({
                "screenPath": clip.screen,
                "webcamPath": clip.webcam,
                "sourceStartSec": clip.source_start_sec,
                "sourceEndSec": clip.source_end_sec,
                "webcamOffsetSec": clip.webcam_offset_sec,
                "hasAudio": clip.has_audio
            })
        })
        .collect();
    Ok(serde_json::from_value(serde_json::json!({
        "clips": clips,
        "layout": {
            "preset": "no-webcam", "webcamSize": 1, "webcamShape": "rectangle",
            "webcamMirror": false, "webcamReactiveZoom": false,
            "screenRect": {"x": 0, "y": 0, "width": 1, "height": 1}
        },
        "effects": {"padding": 0, "blur": 0, "shadow": 0, "roundnessFrac": 0, "motionBlur": 0},
        "background": {"kind": "color", "color": "#000000"},
        "zoomRegions": [],
        "cursor": {
            "show": false, "size": 1, "smoothing": 0, "motionBlur": 0,
            "clickBounce": 0, "clipToBounds": false, "theme": ""
        },
        "output": {"width": params.width, "height": params.height, "fps": params.fps}
    }))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_export_scene_fills_the_output() {
        let scene = full_frame_scene(&[], &ExportParams::default()).expect("native scene");
        let rect = scene.layout.screen_rect.expect("explicit screen rectangle");
        assert_eq!(
            (rect.x, rect.y, rect.width, rect.height),
            (0.0, 0.0, 1.0, 1.0)
        );
        assert_eq!(scene.layout.preset, "no-webcam");
        assert!(!scene.cursor.show);
    }
}
