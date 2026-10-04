# Zeqa regression investigation

The local baseline is `48038ec0`, on `fix/zeqa-regressions`. No live server or remote
build was used. Captures and images remain outside git in the supplied scratchpad's
`regressions/` directory.

## Confirmed defects

| Report | Source introduction | First-parent exposure | Correction |
| --- | --- | --- | --- |
| Nametag material batches skip later tags/lines | `0d7845ac` | `8201d6e0` | A Bevy sorted-phase batch consumes one phase entry. Glyph record ranges are looked up separately by that entry's batch index. |
| Stale nametag glyphs after a server font swap | `3fd0f3c3` (atlas cache) | `faff35a3` | Drop rasterized line cells when installing or removing session glyph sheets; build tags after the frame observes the font. |
| Red/dark-red logo blocks | `4418dffd` | `9d8db17d` | Preserve omitted plane back faces. The native arrow material's explicit two-sided exception remains. |
| A spawned player loses its skin after roster removal | `bef61416` (original actor lifecycle) | `fe698f57` in the current first-parent lineage | Keep the appearance until the matching actor despawns. Skin updates, relisting, replacement and dimension reset retain correct byte accounting. This defect predates today's merges. |
| Bone matrices survive geometry replacement | `0eb325ed` | `0d7041e7` | Clear derived matrices after successfully replacing geometry/pivots. The focused test replaces a rig while retaining the same pose allocation. |

The first-parent search bisected source predicates and checked each boundary's parent;
it was not a full compiled client bisect. The nametag fixes address missing/skipped
material draws and rasterized lines surviving a font swap, not a demonstrated change
in world scale. Existing scale, multiline,
plate-opacity and depth tests remain authoritative implementation checks.

The logo snapshots show the actual reported red rectangles becoming readable lettering:
`before/actors/00_zeqa_npc.logo.merge.png` and
`after/actors/00_zeqa_npc.logo.merge.png` (also `01_zeqa_npc.logo.png`). Texture lookup
was working; an incorrectly enabled back face covered the textured face.

## Offline witnesses and remaining limits

- `scene_report::render_captured_scene` draws 67 tags / 182 records into
  `after/scene.png`. This software witness bypasses Bevy phase traversal and is
  not proof that the GPU nametag defect has been reproduced or visually closed.
- The player report now runs actor preparation before publication. The packet
  replay produces 83 appearance states for 59 spawned players, with textured
  body triangles in every state and no rejected actor packets. See
  `after/players/players.json` and its PNGs. The old blank body snapshots lacked
  the preparation step and are not valid before images.
- The exact captured Spirit Bundle request passes through the packet decoder
  and real JSON-UI carrier. Its settled snapshot has 732 vertices / 11 batches,
  resolved bundle artwork and button textures, with unchanged cache pass counts
  across subsequent frames. See `after/spirit-bundle.png`. The earlier synthetic
  fixture omitted the captured description and was insufficient evidence.
- The captured inventory witness installs the actual production server icon
  compiler before resolving items and publishing JSON-UI draw batches. All six
  occupied slots resolve and draw their server icons in `after/captured-hotbar.png`.
  A fixture without session icons cannot diagnose missing custom hotbar art.
- The provided lobby capture contains no `RustMCBE` name. The rig replacement
  cache defect is independently confirmed; its connection to the stretched-limb
  screenshot is not established. The player report isolates bodies and omits
  armor, held items and GPU execution.

The reported form FPS loss, live form layout, missing hotbar icons, remaining huge/
garbled nametags, and the particular stretched player are not accepted as fixed by
these software witnesses. No visual or performance parity gate is closed here.

## Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Player appearance | Active entities retain their skin after roster removal. |
| Name tags | Use the name-tag geometry, materials and constants recorded in `nametag-rendering.md`. |
| Geometry faces | Distinguish absent faces from authored per-face UVs and read each supplied face’s UV data. The Zeqa logo’s border faces omit the opposite direction; its material is not nocull. |
| Bone matrices | Copy the selected bone matrix to the skinned mesh. Matrix caches must include the current geometry’s pivots. |
| Inventory items | Read the control’s property bag and dimensions before drawing the item. |
| Forms and hotbar | `ui/server_form.json:118` instantiates dynamic form buttons; `ui/server_form.json:166` binds image texture/file system. `ui/hud_screen.json:1212` defines the hotbar collection; `ui/hud_screen.json:1237` defines its item renderer control. |

Pack reference root: the installed pinned vanilla pack under
`.local/assets/bedrock-samples/`; the carrier and original pack were read only.

## Focused validation

The render tests cover native tag scale/materials and one visit per material batch,
omitted catalog backs, and changed pivots under a retained pose allocation. The
client-world tests cover roster removal, skin updates, relisting, actor replacement
and dimension reset. The app font test verifies changed raster allocations and
restored metrics for the same tag across glyph-sheet installation/removal.

The exact form and inventory capture tests pass with the supplied local fixtures.
Neither requires a live connection. Full gate output is kept externally as
`regressions/lcheck.log`; its final exit and commit line establish the gate result.
