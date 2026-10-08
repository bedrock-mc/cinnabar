# Farmland rendering

Farmland's compiler admission used the sequential-ID formula from the legacy
registry. The current target registry reorders blocks, so all eight valid farmland
states failed admission and fell back to diagnostic visuals instead of their
packed models. Existing farmland tests used only the legacy registry.

Admission now requires the complete typed `moisturized_amount:int 0..7` product,
unique sequential IDs and network hashes, and the existing exact family, role,
flags, coverage, and collision evidence. Network identities come from the input
registry, without a version-specific numeric formula. Farmland's geometry,
materials, lighting, and renderer behavior retain their established contracts.

The subsequent manual check confirmed the surface renders, but exposed wheat's
incorrect generic diagonal cross. Wheat now uses the vanilla four-row model,
lowered by 1/16 block to meet the soil. Other plants retain their existing models.

## Vanilla rules

For the `1.26.50.26` client:

- Farmland's visual shape is `(0,0,0)..(1,15/16,1)`.
- The texture variant is whether moisture is below one. The pinned pack binds
  dry to index one and wet to index zero, so moisture zero is dry and all other
  states are wet. Tessellation uses this selector.
- Graphics are built from pack names and registered block types; there is no
  legacy sequential-ID formula.

The older gallery and UV limitations remain documented in
`docs/evidence/phase-2-farmland-native-reference.md`. These rules confirm height
and top selection; no new claim about calibrated source rows is made.

### Wheat above farmland

Wheat uses the row (crop) shape:

- The world entry offsets block Y by `-0.0625` before emitting rows.
- It emits four axis-aligned full-width planes, with a reverse side for each,
  using constants `0.5`, `-0.25`, `-0.5`, `0.25` and `1`. These yield rows at
  X/Z 1/4 and 3/4, spanning 0..1. The resulting Y range is -1/16..15/16 relative
  to the wheat block.
- UVs bind the full sprite upright to each row, with opposing rows reversing
  their world direction. Cinnabar represents the reverse sides with its
  existing two-sided quad flag, rather than duplicate coincident quads.
- The pinned pack's `blocks.json` binds wheat to the `wheat` texture key, whose
  terrain array contains `wheat_stage_0` through `wheat_stage_7`. The existing
  growth selector already selects the corresponding image and stays unchanged.

The former diagonal pair widened the sprite to sqrt(2) block units and started
at block Y, leaving a 1/16 gap above farmland. The new model changes only wheat
geometry and includes geometry in template cache identity, so a plant sharing
its material cannot inherit its four-row model.

## Regression coverage

Compiler regressions load the active registry through `assets/bedrock-target.json`,
cover all eight moisture states and both network ID modes, and verify deterministic
output with reversed input order. An independently reordered synthetic identity
product guards against restoring a numeric ID formula. Malformed state products,
duplicate identities, and invalid source materials remain rejected atomically.

The existing meshing regressions now consume the active target registry and cover
all states, both network ID modes, subchunk boundaries, opaque neighbours,
additional water, and dense uniform/mixed products. Legacy compiler regressions
remain to preserve compatibility.

The focused compiler/pack/meshing run passed all 12 selected tests. The production
`assetc compile` path was exercised with the manifest-pinned pack and complete
target registry/light/biome inputs. Inspection of the rebuilt carrier confirms
that every farmland state has a model: moisture zero selects the dry template
and moisture one through seven share the wet template. The pre-fix carrier had
diagnostic visuals and no template for all eight states.

Formatting, focused compiler/meshing clippy, and the architecture check passed.
The client and all runtime carriers were rebuilt with these changes on `dev`
base `538aecdc`. No networking or authentication source changes are included.

The live target was the local BDS on port 19132. Its configured
transport is NetherNet. The freshly rebuilt client joined the existing offline
LAN session as `RustMCBE` and rendered the world on macOS Metal. The user's
October 4 screenshot and manual report confirm that the farmland surface renders;
that screenshot also supplied the wheat reproduction above. No transport source
changes were needed. The native live witness is PlayCover `1.26.51.01`, a
near-version witness to the target preview, rather than an exact-version gate.

The wheat regression checks every active-registry growth state, both network
identity modes, upright full-sprite UVs, quarter-position rows, unmodified source
pixels, template cache isolation, and deterministic input ordering. A meshing
regression checks soil contact within and across subchunks. All 139 active
compiler integration tests and five farmland/wheat meshing tests pass; nine
opt-in compiler tests are ignored in that run. The corrected client and carriers
were then relaunched on macOS Metal (Apple M3 Pro, 1280x720 content, scale 1).
The user manually tested the farmland and wheat fixes and approved them on
October 4, 2026. Exact version-matched native gallery and two-frame packed-model
presentation acceptance remain open in `plan.md`.

The final integration syncs the latest remote `dev` changes. Further tests after
that sync are skipped at the user's explicit request; the test and lint results
above describe the pre-sync revision.
