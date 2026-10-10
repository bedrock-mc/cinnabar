# Render frame time on DX12

Captures: aeris.land lobby spawn, idle, render distance 32, MSAA 2×, VSync off,
developer-control client driven over client MCP, Tracy 0.14.1, 30 s each after a 20 s settle.
Windows 11, Ryzen 9800X3D, Radeon RX 6600 XT, DX12 (then the Windows default; it is now
Vulkan). Before and after builds, on `a12bc668c`, ran back to back in one session, two captures
each per resolution. Frame numbers come from captures without per-node GPU timing; GPU
attribution comes from `RUST_MCBE_GPU_NODES=1` captures of the same builds.

## Before and after

Means of two captures, ms unless noted:

| Metric | 1080p before | 1080p after | 1440p before | 1440p after |
|---|---:|---:|---:|---:|
| fps | 452 | 681 | 384 | 686 |
| frame mean | 2.214 | 1.468 | 2.603 | 1.458 |
| frame p50 | 2.162 | 1.410 | 2.573 | 1.360 |
| frame p99 | 3.043 | 2.452 | 3.228 | 2.482 |
| GPU frame | 1.991 | 0.727 | 2.376 | 1.089 |
| render thread busy | 2.054 | 1.282 | 2.446 | 1.282 |
| `CommandEncoder::finish` | 0.732 | 0.206 | 0.732 | 0.203 |
| DXGI present | 0.525 | 0.181 | 0.913 | 0.202 |
| queue submit | 0.087 | 0.161 | 0.094 | 0.154 |
| main app | 1.214 | 1.207 | 1.214 | 1.146 |

Submit grows because the worker-encoded passes arrive as separate command buffers.

GPU passes from the node-timing captures (node timing itself adds GPU time):

| Pass | 1080p before | 1080p after | 1440p before | 1440p after |
|---|---:|---:|---:|---:|
| GPU frame | 2.160 | 0.882 | 2.666 | 1.315 |
| main opaque | 0.854 | 0.370 | 1.162 | 0.691 |
| late cull | 0.650 | 0.149 | 0.746 | 0.206 |
| transparent | 0.450 | 0.130 | 0.456 | 0.120 |

Launch to HUD on aeris.land, three joins each with Tracy builds: 23.0, 23.6 and 23.0 s
before; 4.0, 3.6 and 3.9 s after.

## What cost the time

Baseline GPU frame at 1080p, 2.07 ms:

| Pass | ms | Cause |
|---|---:|---|
| main opaque | 0.81 | early culled terrain: fixed-count submission of every slot's worst case (4,100 commands per phase) with wgpu validating and patching each command for DX12 |
| late cull | 0.67 | the same commands again (draws 0.50) plus the Hi-Z pyramid (0.14) |
| transparent | 0.47 | ~230 sorted draws alternating the liquid and model programs; the switch, not the drawing, costs the time |

Measurement-only experiments isolated the causes: with wgpu's indirect validation off the
GPU frame fell to 1.48 ms (incorrect picture, same work); with every transparent draw on one
program the transparent pass fell to 0.09 ms and its render-thread encode from 406 to 204 µs.
Making the two transparent pipelines share rasterizer state alone changed nothing.

Baseline render thread at 1080p, 2.12 ms busy: `CommandEncoder::finish` 0.77 (transparent
encode 0.36, opaque 0.12, late cull 0.06, compute passes and validation 0.19), DXGI present
0.53 (waiting on the GPU), executor 0.18 across ~210 single-threaded render systems, graph
0.15, submit 0.09.

DX12 compiles every pipeline with FXC, which the renderer pins, at every launch; nothing
persists between launches. Each cull kernel took about 7.5 s, nearly all of it spent on the
loop wgpu adds to zero workgroup memory; the same kernels compile in 0.1 to 0.2 s without it.

## Changes

1. Culled terrain commands carry no builtin-visible offsets: the cull kernel writes each
   command's base vertex and first instance into a draw-offset vertex buffer, so DX12 takes
   the count-draw path and submits only compacted commands; wgpu neither validates nor
   patches count draws. Pixel-identical to the CPU path on DX12 and Vulkan for both
   submissions (`gpu_culling` integration test).
2. CPU-planned draws bind the arena's identity offset buffer, so both paths share one
   pipeline per terrain family and DX12 compiles no extra variants.
3. The opaque, late-cull and transparent passes record and encode on worker tasks.
4. Sorted water and model draws share one pipeline (`transparent_terrain.wgsl`); water draws
   flag their first instance. An offscreen GPU test matches the shared pipeline against both
   programs through their own pipelines, pixel for pixel.
5. The Hi-Z pyramid builds only texels that cover a depth pixel; the cull reads no other
   texel. The pyramid fell from 0.14 to 0.11 ms at 1080p and from 0.23 to 0.17 ms at 1440p.
6. The cull kernels skip wgpu's workgroup zero-fill; each kernel writes its workgroup slots
   before reading them.

Live check on aeris.land at 1080p: GPU-culled, CPU-planned and before frames at four
headings differ only in players, nametags, chat and animated entities.

## Remaining

The frame is now CPU-bound at both resolutions, with the main and render threads near
1.2 ms each. Render thread at 1080p: executor bookkeeping 0.18, present 0.17, encoder finish
0.20, the command-buffer generation zone 0.15, submit 0.16; no render system exceeds 0.04 ms.
Main thread: actor render-frame preparation 0.24, UI build 0.17, block-entity scene update
0.10.
