# Projectile rendering investigation

The fixes in `fix/projectile-render` address invisible item sprites, undersampled arrow
textures, discarded arrow plane backs, duplicated world yaw, and dropped remote motion.
They do not close the vanilla projectile parity gate.

## Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Face UV defaults | Face dimensions are initialized from caller defaults before an optional `uv_size` read. Omitted face UV dimensions use the authored cube face size, shared between UV validation and mesh generation. |
| Camera rotation | `query.camera_rotation(0/1)` reads camera pitch/yaw. Billboard bones retain the pack's camera rotation, without an additional mob body yaw. |
| Actor yaw | Four actor types read wrapped, interpolated absolute actor yaw; mobs read bounded relative yaw. Arrow target yaw is absolute, and world placement does not apply it again. |
| Actor classification | Actor registrations identify arrow `0xc00050`, fireworks rocket `0x48`, wither skull `0x400059`, dangerous wither skull `0x40005b`. One classification supplies both query evaluation and world placement. |
| Interpolation | The classic movement route starts interpolation with at least three steps. Existing three-tick interpolation is retained. Projectile prediction/component selection still needs confirmation. |
| Motion | Throwable and abstract-arrow motion handlers replace retained velocity. The arrow initializes zero prior pitch/yaw from its motion vector. Remote `SetActorMotion` reaches the store; an unrotated arrow initializes its launch angles without moving its position. |

The pinned resource pack is selected by `assets/vanilla-source.json`. Its
`models/entity/item_sprite.geo.json` declares an 8×8 UV frame on an 8×8×0 cube,
origin `[-4,-2,0]`: 0.5 blocks square, from -0.125 to 0.375 blocks vertically.
The entity definition directly binds its item icon, such as
`textures/items/ender_pearl`, rather than an inventory atlas lookup. The actual
icon raster can be 16×16. The compiler previously excluded item texture paths,
required equal declared/raster dimensions, and rejected the unused box UV envelope.
These were independent barriers to publishing the sprite artwork.

The pack's arrow geometry has two crossed shaft planes and a cap. Shaft face UVs
omit `uv_size`; their default region is 16×5 texels. The arrow animation supplies
pitch/yaw and axis scale `[0.7,0.7,0.9]`. Item sprites use
`animation.actor.billboard`; the compiler continues to load the pack's geometry,
texture selection, and animation rather than synthesizing replacements.

The neutral catalog profile already declares two-sided rendering. Its mesh builder
now supplies front UVs for a missing back face, while retaining explicit opposing
UVs and leaving the separate skin mesh builder unchanged. This repairs arrow backs
within that profile. Exact target arrow material and lighting remain unverified.

## Verification

Regression tests were executed against unfixed production code and observed failing
before each corresponding implementation change. They cover item icon admission,
raster resolution, sprite UV bounds, default arrow face UV size, arrow back UVs,
absolute arrow target yaw, duplicate world yaw, remote motion ingress, and initial
arrow motion rotation. Later motion must retain the existing orientation and position.

`projectile_report::render_projectile_states` uses the real compiled vanilla geometry,
artwork, actor poses, presentation adapter, and scene publication. `scene_report` then
rasterizes the published triangles offline, including backface UV selection. The
fixed-camera gallery covers arrow flight, an arrow at a textured stone block, pearl,
and snowball. This is a CPU frame witness, not target-platform GPU acceptance.

Rebuild the actor carrier with `make assets` before using the fixes with previously
compiled local assets. This work does not write the owner's `.local` directory.

## Open parity checks

- Exact projectile network component selection: the classic movement route has a
  three-step minimum, matching the existing default. Predicted movement uses a
  separate buffer and actors without an interpolator can snap; projectile-specific
  selection is not yet confirmed.
- Frame-level rotation fidelity: the shared GPU rig interpolates posed matrices;
  vanilla target queries interpolate angles. Dedicated projectile frame validation
  remains open.
- Arrow collision/stuck-state transition and shake trigger/countdown. The pack shake
  animation exists; retained client shake state is not implemented. Configuration
  parsing is not proof of the runtime hit trigger.
- Tipped-arrow rendering: the pack controller has no tint expression, but builtin
  tint or particle behavior has not been excluded with target client evidence.
- Initial AddActor velocity-only launch orientation and special arrow facing flags;
  the measured SetActorMotion initialization is implemented.
- Potion glint, exact target material/lighting, and native target-platform frames.
- The unused sprite back box UV samples outside the declared width; the pack's
  billboard exposes the correctly textured front. Exact backside behavior is open.

No live server connection was used and no parity gate was marked complete.
