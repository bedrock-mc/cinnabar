# Native carried block textures

## Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Overlay colors | Keep the low 24 RGB bits, normalize by 255 and force alpha to one. |
| Overlay mask | For normalized source color `C`, source alpha `a` and overlay `T`, use `mask = a*T.a` and `RGB = C*(1-mask) + T.rgb*C*mask`. Positive overlay alpha makes the face opaque. |
| Output quantization | Multiply normalized output by 255 and truncate. |
| Mip construction | Pass the parsed overlay from tile offset `0x30` to atlas mip construction as overlay argument 15. |
| Sampling | Retain RGBA channel order. |
| Carried grass | Pinned `blocks.json:2748–2753` declares carried faces; `textures/terrain_texture.json:61–70` selects a precolored grass top, dirt bottom and side with authored overlay. Do not borrow biome tint. |

## Correction and verification

Previously both paths rejected the side's overlay metadata: inventory fallback produced
no icon and held-sheet construction refused tinted world materials. The compiler now
retains reviewed carried overlay metadata and resolves six colored tiles once. Those
tiles feed both the thumbnail and an opaque six-face sheet. A bounded, validated carrier
extension binds sheets to block visuals; legacy sprite-only carriers remain readable.
Runtime sheet admission checks manifest identity and the entity carrier's visual bounds.
Unknown metadata, malformed colors, animations and unsupported geometry remain refused.

The compiler's missing-grass regression failed before correction, as did the runtime
carried-sheet admission regression. Five compiler carried regressions, six icon-carrier
tests, 46 pack-parser tests, 30 equipment tests and pinned-pack grass coverage pass.
Workspace all-target tests, formatting, strict Clippy and the architecture policy pass.
The rebuilt local icon report has 14 carried-sheet bindings and no unresolved grass item.

Live macOS 26.3 / M3 Pro / Metal verification used the canonical optimized-debug executable,
logical window 1280x752, rendered content 2560x1440, Retina scale 2. Ignored frames
`.local/screenshots/2026-10-01_12.08.15.png` and `2026-10-01_12.09.02.png` show the
grass hand/hotbar and open inventory: opaque green top/fringe, brown untinted soil,
legible item label, correct slot clipping, and unchanged held-cube camera pose.
Native window capture also verified the open inventory; local macOS input was the explicit
fallback after freshly discovered native input failed its focus guard.

Changes remain local and uncommitted. Inventory-thumbnail shading is still provisional,
and complete held material/lighting and custom geometry/display parity are incomplete.
The two reported bugs are functionally live-tested; no broader vanilla parity gate is closed.
