Noni native export worker

This helper uses the unmodified native compositor from Reaction Station
(OpenScreen), with the accompanying Noni worker entry point.
Source: https://github.com/Clatasha/Reaction-Station/tree/a1a45d8a3d20fb7980700841b845bf6679e00aa8
See Reaction-Station-LICENSE.txt for the upstream copyright and MIT license.

The shared FFmpeg libraries are the pinned BtbN LGPL build used by that
compositor. The DLLs are separate files and may be replaced independently
with ABI-compatible builds. The exact upstream build recipes and source
revision are recorded by the pinned provisioning script:
https://github.com/Clatasha/Reaction-Station/blob/a1a45d8a3d20fb7980700841b845bf6679e00aa8/scripts/fetch-ffmpeg.mjs
Build release: https://github.com/BtbN/FFmpeg-Builds/releases/tag/autobuild-2026-07-31-14-10
Build recipes: https://github.com/BtbN/FFmpeg-Builds/tree/autobuild-2026-07-31-14-10
FFmpeg source: https://github.com/FFmpeg/FFmpeg/tree/9b6c8969e0
