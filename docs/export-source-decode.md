# Shared source decoding during export

GPU exports using several drawing workers can prepare file-video pictures
on a single ordered producer. Each event has one source reader across the
workers, including a separate owner for Frame Blend. Pictures are handed to
drawing workers through `Arc` references; no extra pixel-buffer copy is made.
Each worker retains its own effects, compositor, still/generator readers and
fallback readers. The producer prefetches all source layers before collecting
them, preserving parallel decoding of different events.

Input queues hold one prepared frame per worker and output queues hold two
drawn frames. Cancellation or a writer error closes the output receivers,
then workers close their input receivers, releasing the producer before thread
joining. The producer never caches a whole video.

Prepared pictures match media ID, video settings, event owner and the complete
request (time, dimensions, decoding form, span and video levels). A mismatch
uses the ordinary reader; it cannot return a nearby or stale picture. Motion
blur, 3D composition and optical-flow frames use ordinary readers. Shared
preparation is selected for progressive packed GPU output with matching frame
shape. Single-worker exports already use a single set of readers.

Set `NONI_SHARED_DECODE=0` before launching Noni to compare the previous
per-worker-reader path. The default is enabled for eligible parallel exports.
The existing `NONI_EXPORT_TIMING=1` report includes drawing-worker count,
prepared frames/pictures, exact prepared-picture requests reused, and source
preparation time. Preparation runs concurrently with drawing and writing, so
its cumulative time must not be added to those stages for wall time.

Tests cover exact-picture reference reuse, changed request rejection, ordinary
reader retention, one source read per frame, ordered preparation/output,
cancellation and writer failure. Windows NVIDIA hardware benchmarking remains
necessary to measure speed and confirm output on the target GPU.
