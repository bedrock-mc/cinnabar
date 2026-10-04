# Native animated held items

The runtime pack is selected by `assets/vanilla-source.json`.

## Modern attachable route

`ItemInHandRenderer` skips its legacy icon placement
when an active attachable is present. First-person rendering evaluates the owner’s
skeleton first. The attachable's scripts, controllers,
render controllers and animations then select its own geometry, texture and pose.
`setupAttachableNoChecks` composes the parent's matrix before the
item's channels. Treating this as the third-person sprite grip was the bow bug.

The compiler now retains attachable scripts/controllers, bone bindings and
`texture_meshes`, plus all referenced texture frames. Runtime evaluation reuses
the actor animation engine with explicit first-person context, owner-variable
inheritance by name, owner lifetime and frame interpolation. Item duration queries
are ticks in this context; ordinary actor query units are unchanged. The parent
hand is interpolated once, and the already sampled item pose is not sampled again.
The owner-skeleton camera root also retains the native post-scale 1/128-model-unit
vertical lift; the ordinary camera-space sprite/block path does not inherit it.
`c.item_slot` remains a string (`main_hand` or `off_hand`), not a boolean or
number. Binding setup converts an authored bound-root Y pivot to pivot minus 24 model units. This
is what makes the vanilla crossbow and shield roots meet their owner's hand;
an item-specific screen offset would hide the underlying bound-root error.

## Texture-mesh frame

Native `TextureTessellator` admits pixels with alpha at least 2.
Its raster points are `[column, depth, row]`: the image is in the X/Z plane, not
the ordinary sprite's X/Y plane. Depth is `max(image width, image height) / 16`
when `use_pixel_depth` is enabled, otherwise 1. Both sides sample the pixel-center
UV; there is no UV-row reversal.

`compileQuads` composes position minus the authored bone pivot, Z/Y/X Euler
rotation, negative local pivot, and texture/model scale, in that order. The loader performs the bone-pivot subtraction. Native model Y is converted
to our upward-Y rig together with the existing X mirror. This conversion belongs
to the texture mesh, not a new global mirror for all held items. The serialized mesh includes scale and a default-enabled pixel-depth field.

## Bow and crossbow timing

Bow construction identifies use animation 4, not legacy branch 5.
Its animation frame comes from `RangedWeaponItem`: for elapsed
use seconds `s`, power is `min((s*s + 2*s)/3, 1)`. While using, the frame is
`truncate(3 * power * 0.99) + 1`; otherwise it is zero. The transitions occur at
elapsed ticks 9 and 15. This differs from the bow pack's ten-tick pose-charge
expression: texture frame and pull-pose progress must not be conflated.

The pack supplies standby/pulling geometries, all four texture frames, the
first-person wield rotation/offset, pull rotation/offset and full-charge shake.
These are consumed from the pinned pack rather than restated as Rust constants.

Crossbow uses its Quick Charge-adjusted duration with the same
quadratic power, but `truncate(power * 0.99 * 5)` and no bow-style increment.
Loaded arrows and fireworks select distinct frames; the firework branch can
select the last frame during the final charge interval. Boundary regressions
cover unloaded/loaded, Quick Charge and firework cases. Trident and shield
authored transforms/controllers enter the same attachable pipeline rather than
the bow's pose or a generic sprite rotation.

## Verification and explicit gaps

Compiler/asset tests cover retained bindings, mesh fields, frame textures and
backward-compatible carriers. Runtime tests cover first-person context, tick
queries, frame interpolation, owner variables across different catalog layouts,
geometry/texture selection and parent composition. The workspace all-target
suite, formatting, strict Clippy and architecture policy check passed locally;
the canonical debug client and ignored carriers were rebuilt. A live functional
pass on macOS 26.3 / Apple M3 Pro / Metal at Retina scale 2 used actual
server-supplied vanilla items in an isolated creative loopback world with fixed
noon/clear weather. Bow idle, partial draw, full draw and release were captured
at `2026-10-01_20.57.13` through `20.57.17` beneath ignored `.local/screenshots/`.
The authored extrusion is visible with opaque colored faces, the charge changes
geometry/pose, the item returns to standby after release, and the fired arrow
is visible. Geometry, edge clipping, layering, scale, color and live right-button
input were inspected; the expected bow geometry remained visible through the sequence.
Native capture used ScreenCaptureKit; native input's `window_not_focused` error
required a PID-scoped input helper, with fresh captures verifying each result.
This is a functional rendering/input pass, not a controlled version-matched
native frame comparison or a complete visual parity gate.

Incomplete: multi-layer/enchantment materials, custom binding-expression parents,
offhand submission in the application, and nonuniform parent/child shear.
The inherited actor engine's controller/query and
Molang tolerances remain subject to its existing parity gaps. Legacy mirrored-art
branches, custom first/offhand render offsets, map placement and exceptional
non-cube block transforms are separate identified paths, not implemented by this
modern bow correction. No claim that every held-item edge case is complete.

## Session item registry initialization

The packet handler calls the `ItemRegistryRef` wrapper, which invokes
`ItemRegistry::matchServerItemIds`.
Its initialization state at offset `0x331` gates execution: state 3 returns
without changing the registry, and successful initialization finishes in state 3.
Repeated packets are not runtime registry replacements.

Dragonfly sends the full item table during StartGame and then a custom-only
registry after spawning; with no custom items the repeat is empty. Keeping that
repeat as a replacement erased inventory-search names and held-item resolution.
Login now retains the first table and shield ID; common play ingress decodes
repeats for wire validity but never publishes them to either item consumer.
Lower-level registry-rebind/recovery utilities remain unchanged. Encrypted
regressions cover empty and nonempty repeats and fatal truncated repeats with
the chunk cache both disabled and enabled.
