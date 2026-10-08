# Loading-screen texture residency

The loading screen must paint against the texture pages it publishes. A completed
artwork atlas is installed before painting; server-page writes made during painting
are uploaded afterward without installing another artwork atlas. Superseding an
artwork request discards its prepared result. Replacing a pack clears its old art
references, and changed bytes under the same texture key invalidate decoded artwork.

Cold texture misses decode in increasing source area, with a stable key tie-break.
The decode budget starts after source lookup. This lets small backdrops and animation
strips become resident before a large logo consumes the inline budget.

## Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Tiled sprites | Read `tiled_scale` and multiply tile dimensions by it. |
| Gradients | Read direction and two colors; select vertical or horizontal drawing. |
| Flip books | Read frame parameters when creating the animation. |
| Loading text | Select the building-terrain message during world generation. |

- The pinned vanilla pack's `ui/progress_screen.json:1215` declares the 2× tiled
  overworld dirt backdrop and black gradient alpha 0.5 to 0.7.
  `ui/ui_common.json:2244` supplies the tiled image base. The flip-book at
  `ui/progress_screen.json:531` uses ten 64×8 cells at 10 fps, reversing direction,
  with a 0.7 tint. `texts/en_US.lang:8153` and `:8179` supply the screen's wording.

## Regression evidence

`forms/loading_texture_tests.rs` renders through the real carrier and local pack.
The Zeqa fixture checks cold dirt residency, actual dirt pixels and gradient
darkening, identical screen pixels across an artwork repack and pack reload, and
same-key replacement with different artwork bytes. A synthetic green artwork makes
an incorrect region obvious: restoring the old publication order changes both the
title and loading-bar area to green. The cold-atlas test fails with the old decode
order because dirt is not resident after the expensive title decode.

Zeqa's actual `textures/ui/loading_bar.png` is fully transparent. The unmodified
pack must keep that override; the small figures in the owner's corrupted frame
are unrelated atlas pixels. A separate offline fixture removes only that override
and checks vanilla bar pixels and animation. No pack files are changed on disk.

The restored Zeqa archive set supplies both the asset and UI layers through
`CINNABAR_FORM_PACK_DIR` (colon-separated, lowest first). Pack files and screenshots
remain outside git.

The art-page publication issue dates to `f4a4d802`, with the asynchronous end-of-frame
installation added in merge `ad949ed8`. Unordered misses became visible omissions
with the decode budget in `ea2ec797`. The worker in `153d8ebe` retained superseded
prepared results and cached pack artwork by key rather than payload; live reloads
in `c1381a21` exposed the latter. No separate fault in `ec9b3731` was established.

The CPU snapshot rasterizer now interpolates vertex colors, so gradient assertions
measure the same interpolation as the UI shader. Previously it used the first
vertex's color for an entire triangle. Offline evidence does not close a live
visual parity gate or prove how long the owner's black frame persisted.

## Artwork retention under cancellation

The content-hash cache keys introduced by `646b710b` preserved every replacement
image when a batch was superseded before the worker's final `trim`. The source
bisect identifies that commit; it reaches the originally investigated `18f3509a`
through that commit's second parent. The preexisting worker could also accumulate
obsolete distinct paths when requests continually interrupted packing.

Eviction now runs after every completed decode batch, before cancellation can
skip it. A changed payload replaces the previous cached image at the same path
and size. The existing 160-entry limit remains in force. Tests churn 2,000
same-path generations and 2,000 distinct paths, and verify the newest pixels.

An isolated cancellation soak replaces one 64x64 texture 2,000 times. Baseline
`199e0856` keeps one image (16,384 pixel bytes, final RSS 2,896 KiB), current
`18f3509a` keeps 2,000 (32,768,000 bytes, RSS 35,968 KiB), and fixed code keeps one
(16,384 bytes, RSS 2,720 KiB). This deliberately takes the worker's superseded-batch
path without waiting for final packing. Harnesses and logs are under
`/private/tmp/cinnabar-growth/`.

Vanilla uses a bounded texture cache with an eviction callback and explicit
texture replacement/unload. This supports resource lifetime management, not an
assertion that vanilla uses Cinnabar's 160-entry budget.
The pinned vanilla 1.26.50.4 pack's `ui/hud_screen.json:949` destroys completed
chat factory controls, and `ui/server_form.json:25` creates form controls through
its factory. Cache eviction changes neither those control lifetimes nor screen
geometry, pixels, texture identity, or alpha handling.

## Large-image decode concurrency

`f4a4d802` added float RGBA conversion for correct premultiplied downscaling.
That scratch allocation is larger than the image reader's decoded-byte limit.
The existing eight-image Rayon batch could hold eight such intermediates at once,
on top of the world worker pools and Bevy's task pools. Decoding now stays on the
already dedicated artwork worker; image conversion and output pixels are unchanged.
A cold batch may finish later, while rendering continues on the previous atlas.

The matched offline eight-image fixture uses 2048x2048 PNGs and identical image
and Rayon versions. Sampled peak RSS is 430,816 KiB on `199e0856`, 923,888 KiB on
`18f3509a`, and 134,752 KiB after serializing decoding. Sampled process thread
counts are 14, 14 and 2 respectively. All three produce eight images totaling
33,423,488 pixel bytes. These are isolated decoder process measurements, not
whole-client thread counts or proof of a GPU-driver hang. The vanilla
texture image cache is resource-location keyed; no authored UI or texture selection changes.

## Startup page insertion and live-order replay

`67041c853` inserts the optional OreUI originals page after JSON-UI initialization.
That moves all dynamic slots, but the engine's fallback/server page base previously
kept the old index until a server pack was applied. Before that pack arrives, dirt
and the loading strip sample the preceding slot (the last glyph page). The slot
base now follows the new dynamic start during insertion.

`vanilla_loading_before_pack_arrival_survives_static_page_insertion` fails before
this change on actual rendered dirt pixels, with a black backdrop, and uses a
synthetic static page so it does not require an OreUI install. This establishes a
pre-pack defect; it does not establish the cause of the supplied post-pack Zeqa
screenshot.

`zeqa_lazy_pages_survive_menu_join_reload_and_cancellation` mounts the real fixture
textures through the production lazy archive reader, warms menu art, joins, cycles
four pack generations and supersedes artwork requests during the first twelve
frames of each generation. Every frame checks dirt/gradient texels, and settled
frames must be pixel-identical. The native Metal replay
`zeqa_late_pages_match_the_published_frame_on_gpu` compares grid samples against the
CPU publication while pages, glyphs, skins and pack ownership change. The existing
bar test independently checks Zeqa's fully transparent override.

The supplied Zeqa page-grid corruption and 6 FPS did not reproduce in these
replays. Current logo, backdrop and bar snapshots are correct; a live 1:1 parity
and performance gate remains open. There is no evidence here to identify a later
merge as reintroducing `646b710b`'s fixed publication-order defect.
