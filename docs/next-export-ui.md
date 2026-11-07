# Export and toolbar update

The everyday MP4 exporter separates named presets from the three quality
radio choices. Presets save codec, resolution, frame rate, bitrate and audio
settings in the existing interface settings file. Names are trimmed, bounded
to 80 characters, unique ignoring ASCII case and limited to 64 presets. The
pencil renames a preset without changing its saved settings; the existing bin
icon deletes it. Changes to settings return the selection to Custom. Failed
persistence restores the previous in-memory preset collection and displays an
error. The Settings button opens the bitrate and audio settings window.

The exporter offers VBR and CBR. CBR requests target and maximum at the same
bitrate and uses Noni's encoder if the alternative native exporter cannot
honor it. The export footer shows the selected duration and estimated decimal
MB from video plus audio bitrate on the left, with Export on the right. Loop
range and setting changes update the estimate. Actual VBR size and container
overhead can differ. The window's close control replaces the Cancel button.

The timeline's right controls are Record, Canvas Ratio, Save Snapshot,
Snapping, Automatic Crossfades, Auto Ripple, Lock Envelopes and Listening
Volume, followed by a separator before the cursor fields. Snapshot uses the
existing camera icon. Ratio and file snapshot leave the preview toolbar.
The speaker opens a longer vertical listening-only slider on hover. A grace
period lets the pointer cross onto the panel, and dragging keeps it open.
All three toolbar time fields use the bundled Oswald timecode renderer with
stable numeric spacing and physical-pixel alignment. About credits Clatasha.

Decoded planar source uploads use wgpu 30's StagingBelt. Every full or partial
command encoder that consumes its memory calls finish_and_recall_on_submit
before submission. The staging buffers are remapped only when GPU work has
completed, retaining copies and lifetime synchronization. Set
NONI_UPLOAD_BELT=0 before launch to compare the former per-upload allocation
path. Export timings count belt uploads versus all decoded uploads. Quality
and encoding settings do not change. Target-GPU speed still needs measuring.

Tests cover preset names, rename, serialized settings, old-setting defaults,
estimates, transactional persistence failure, toolbar/icon inventory and
headless layout snapshots using the actual egui meshes and font atlas. The GPU
parity test exercises repeated 8/10-bit uploads to catch stale data when a
supported adapter is available. Screenshots of other editors remain local;
this implements the user's requested standard controls with Noni's own icons.
