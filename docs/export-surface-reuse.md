# Reusing shared GPU output surfaces

The compositor's D3D11-originated NV12 textures can be reused by direct NVENC
exports after both their D3D12 copy and encoder consumption complete. This
avoids creating, sharing, importing, initializing and flushing a fresh GPU
texture for every output frame. Encoding settings, effects and quality do not
change.

The GPU owns a pool of at most 24 allocations across sizes and painters. Each
output frame and native-copy completion callback retains the allocation by
`Arc`. Direct NVENC retains a completion callback in the corresponding encoder
slot before issuing the input copy. After the encoder event, bitstream retrieval,
unlock and unmap all succeed, it invokes that callback to arm recycling.
Only an armed allocation whose sole remaining owner is the pool can be reused.
A new checkout disarms it. Abandoned callbacks never arm it: failed, cancelled,
probe and Media Foundation frames are discarded. A full busy pool uses a
single-use allocation so it cannot deadlock the encoder waiting for a surface.
Free allocations of other sizes can be evicted when the pool is full.

Set `NONI_NV12_SURFACE_POOL=0` before launching to compare the previous allocation
path. Export timings report reused and newly allocated shared output surfaces.
These differ from the existing private encoder input-surface pool counts.

Portable retirement tests cover both GPU users retaining a surface, successful
completion, repeated checkout, abandoned encoder callbacks, and single-use
system-encoder/probe frames. An ignored Windows D3D12 test checks actual pool
identity and reuse; the existing queued-color test checks native-copy lifetimes.
Hardware execution and export speed/quality checks require a supported Windows
GPU and are not established by cross-compilation.

The timeline time display separately uses bundled Oswald Medium at up to 24
points. Numeric digits use fixed-width cells and each glyph origin is snapped
to physical pixels. The font is rasterized at its display size, without scaling
a rendered text image. Headless checks cover widths and layout at 100%, 125%,
150% and 200% DPI. The rest of the interface keeps its existing fonts.
