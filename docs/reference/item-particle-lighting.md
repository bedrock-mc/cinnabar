# Dropped-item and particle lightmap composition

Runtime particle definitions come from the pack selected by `assets/vanilla-source.json`.
These vanilla contracts are implemented in our own Rust/WGSL.

## Lighting contracts

The shared in-hand item renderer draws ordinary dropped items (see
[dropped-items.md](dropped-items.md)). Its actor lighting setup supplies
`(sky, block) / 16` to the lightmap lookup, which samples normalized byte
RGB with clamp-linear coordinates `16 * uv - 0.5`. There is no integer texel
shortcut or gamma-to-linear transfer inside this lookup. Cinnabar's existing
`actor_light_colour` reproduces the lookup with a transposed table: block in
the low nibble, sky in the high nibble.

Modern pack particles use the same RGB lookup. Each particle-engine tick
builds a 16-by-16 gameplay-light cache by looking up each pair of nibbles
multiplied by `0.0625`. A particle retrieves the four-channel cached entry for
its brightness pair, which floors world particle coordinates,
including the emitter translation for local-position effects. The pinned
`block_destruct` effect enables `minecraft:particle_appearance_lighting`.
An effect without that component bypasses gameplay light.

The appearance-lighting component multiplies cached gameplay RGB into render
color, leaving alpha untouched.

The ordinary vanilla material color contract composes normalized gamma RGB;
the existing actor pipeline applies the same contract. Bevy samples
our sRGB atlas as linear RGB and writes an sRGB target, so the item/particle
shaders undo the atlas decode, multiply vanilla tint and RGB lighting, then
convert the completed color to linear once at output. Item overlays precede
the lighting product and item distance fog also composes gamma RGB.

## Corrected data path

Dropped items now use the byte-quantized `/16` lookup and vanilla color
composition instead of multiplying gamma lightmap RGB into decoded texture
RGB. The latter caused an extra sRGB encoding of the dim-light multiplier.

Lit particles publish independent block/sky nibbles and a lighting flag in
the existing instance record. Their GPU pipeline binds the shared world
lightmap, so environment, brightness and vision updates affect existing
particles without a separate CPU brightness curve. The old fixed ambient
floor, minimum night sky scale and scalar maximum of the channels are gone.
Authored particle tint stays in gamma RGB until the shader completes the
texture/tint/light product. Unlit effects retain their bypass.

## Verification and remaining boundaries

`item_particle_lighting` exercises the production item and particle vertex
and fragment functions on an sRGB GPU target, comparing pixels against the
vanilla byte lookup in darkness, daylight, sky light at night, torchlight,
minimum/maximum brightness and night vision. Particle draw-list regressions
check separate channels, dark samples, level clamping and unlit admission.

The October 4 user-authorized scratch-BDS run used macOS/Metal on Apple M3 Pro,
2560 × 1504 rendered pixels at Retina scale 2. The user first confirmed the
corrected breaking particles, then accepted dropped items and survival breaking
after the capability correction.
This is a live functional/color acceptance of this correction, not a full
version-matched vanilla scene or performance parity gate.

This correction does not close complete item or particle parity. The existing
provisional dropped-item normal shade still needs separate sprite-versus-block
material handling. Vanilla item AABB brightness sampling is not replaced by
this correction. Particle solid-cell sampling selects the
brightest of six axial neighbors; that fallback remains incomplete in our
point-sampling world adapter. Particle fog/material blending and unrelated
geometry, animation and destruction contracts retain their existing gates.
