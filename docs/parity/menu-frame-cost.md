# Menu frame cost

The native-model path added by `ec9b3731` hashed the same skin twice per frame,
once for the preview and again for the retained GUI texture. The fallback skin is
large enough to make this visible during loading. The GUI texture now compares
retained pixels before hashing and returns its stored digest to the preview.
Changed bytes still replace the texture; equal bytes in a different allocation do
not. Source dimensions, model geometry and rendering are unchanged.

`live_pose_changes_only_geometry_and_original_skin_keeps_its_density` checks
unchanged allocations, changed pixels at the same address, native dimensions and
session retirement. `menu_frames_on_native_gpu` renders the real startup carriers,
launcher skin, animated preview, menu art, panorama and UI pass on Metal. It samples
30 frames after 10 warm-up frames per screen at 1280×720, DPI 1. Loading exercises
the fallback skin before a player profile arrives. GPU wait and readback are
included; these are optimized test timings, not release FPS or vsync acceptance.

Measured milliseconds, before → after:

| Screen | Mean preview/art synchronization | Median whole offscreen frame |
| --- | ---: | ---: |
| Home | 0.107 → 0.008 | 3.596 → 2.542 |
| Inbox | 0.109 → 0.006 | 2.920 → 2.058 |
| Play | 0.097 → 0.006 | 2.815 → 2.070 |
| Servers | 0.100 → 0.005 | 2.993 → 2.057 |
| Edit Server | 0.108 → 0.005 | 3.200 → 2.046 |
| Settings | 0.113 → 0.007 | 4.024 → 2.805 |
| Zeqa loading | 4.706 → 0.023 | 8.171 → 2.350 |

The first after run encountered GPU wait/readback times of 18–30 ms and whole
frames of 22–35 ms; repeating after the concurrent checks finished produced the
table. This shared-machine variability prevents attributing the whole-frame
improvement entirely to the hash change. Texture/vertex counts stayed unchanged.
The reliable eliminated work is the pair of redundant skin hashes.

`idle_screen_costs` isolates publication over the real carrier at 2560×1440,
DPI 2. Median build costs were Home 0.37 → 0.35 ms, Inbox 0.01 → 0.01,
Play and Servers 0.02 → 0.02, Edit 0.06 → 0.06, Settings 0.59 → 0.58.
These values do not reproduce the reported 78/24.7 FPS. No full per-frame
re-resolve, art decode or large tree expansion was demonstrated in this fixture.
The existing user's vsync setting/CLI policy is preserved. A release run with the
owner's account, window and load is still required to close the FPS report.

References: the existing source-texture contract is documented in
`docs/reference/inventory-gui-geometry.md`. Vanilla texture retention uses explicit eviction.
The vanilla pack's `ui/progress_screen.json:1215` defines the backdrop used in the
loading benchmark. Caching the digest changes no authored geometry or appearance.
