# Name-tag rendering rules

## Current actor labels

Actor nametags use Java Edition rendering as an accepted deviation. All labels share
camera-right and camera-up, including the front-view pitch sign, with a fixed
`1.6 / 60` blocks per font pixel. The head anchor uses Java's fixed `1.8`-block
standing height for players, so Bedrock's crouching collision height does not lower
it again. Other actors use their published height. Height plus `0.5` blocks of
clearance follows the actor's Scale metadata. Sneaking players lower it by
`0.125` blocks and move the quad `0.25` blocks along
camera-down; sleeping players lower their anchor by `1.5` blocks.
Each line's black plate has alpha `0.25`, one pixel of side padding,
and vertical bounds `-1 .. 8`. The existing ten-pixel multiline spacing and
`0.125` world lift per extra line stay unchanged.

Ordinary labels draw a plate and full-opacity text through geometry so server
cosmetics extending above the actor's published bounds cannot obscure the name. Each complete label is drawn back to front by camera
depth
of the final lifted plane, including cosmetic scale, before starting the next label.
Only lines that fit the scene and atlas contribute to the lift. Viewer distance still
controls visibility and score range. Sneaking labels draw only a depth-tested plate and dim text.
Sneaking text alpha is `32 / 255`; only sneaking text writes depth. Labels have no depth
bias, and sampled atlas coverage must exceed `0.1` before text alpha and cosmetic
opacity are applied. Distance from the viewer's feet
must be below 64 blocks, or 32 while sneaking; server limits can shorten this.
Invisible actors, mounts carrying passengers and server hide flags suppress labels.
The local label stays hidden by default; the existing optional own-tag style is
limited to third person. The open font, server text and formatting, multiline
handling, debug text and explicit debug quaternions retain their existing paths.
Rendered-frame acceptance remains pending.

## Earlier Bedrock implementation

The remaining sections describe the earlier renderer and its incomplete acceptance
work. Its facing and material choices are superseded for actor labels by the rules above.

### Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Name extraction | Extract actor name tags, then apply anchor, render-distance culling and material selection. |
| Text geometry | Measure line widths, build the background, and apply billboard rotation, world scale, multiline lift and centered text. |
| Text materials | Select normal or depth-tested text and background materials and route name tags through environmental text. |
| Depth state | Translate material depth test/write/comparison and apply the environmental-text runtime depth-bias override. |

The original behavior was a screen-space/HUD approximation: quantized font scale clamped
to the HUD's minimum/maximum, a one-line plate behind multiline text, a separate Java-style
score renderer, and collision-ray whole-tag occlusion. The merged upstream path retains the
shared UI font-layout cache and glyph-line atlas, publishing compact font-pixel records to
the environmental-text GPU renderer. The shader constructs homogeneous world quads for
perspective-correct glyphs and near-plane clipping. Actor nametags deliberately use exact
yaw and pitch toward the camera eye instead of vanilla's cubic angle approximation.
Facing uses the base anchor before multiline lift, and a directly vertical eye direction
uses zero yaw. Primitive-shape debug text retains its cubic facing.
World tags render before first-person hands and HUD. Plates do not write depth;
glyphs do. Sneak tags additionally test world depth, while ordinary tags use Always.

### Verified geometry and constants

| Rule | Behaviour |
| --- | --- |
| Ordinary black background | RGBA `(0, 0, 0, 0.25)` |
| Head clearance above the AABB height, both scaled | `0.7` blocks |
| Font world scale | `1.6 * (1/60)` blocks/design pixel |
| Line pitch | `10` design pixels |
| Background top and horizontal padding | `-1`, `1` design pixels |
| Extra world lift per additional line | `0.125` blocks |
| Sneaking foreground alpha | `0.125` |
| Billboard zero-horizontal fallback | `0.0001` |
| Cubic acos coefficients | `0.87266463`, `-0.69813168` |
| Below-name score distance squared | `<100` blocks squared |

The tag anchor sits at the feet plus the published box height (data 54) and the clearance,
both times the Scale data (38), so the whole offset follows the actor's size. A published zero
height is a size, not a missing value. The client applies Scale to the published box as it does
for collision: PocketMine-MP's `Entity::syncNetworkData` publishes its box divided by its scale,
and GeyserMC's `ArmorStandEntity#getBoundingBoxHeight` leaves the published height unscaled
because "the SCALE entity data handles that for us". The clearance scaling follows the user's
observation of the vanilla client, where a falling-block text host at near-zero scale shows its
tag at its base; no client trace fixes it yet. A falling block's network offset likewise follows
its scaled box (`0.5` for the vanilla `0.98` box), consistent with Dragonfly's falling-block
text entity, which sends its authored position with no offset. Without a published height,
`1.8` (`1.5` sneaking) stands in provisionally.

For each nonempty LF-separated line the native font width is halved with integer truncation.
Every line starts at its own negative half-width. Each line gets its own plate,
padded one design pixel beyond its own half-width on each side and spanning
`10 * index - 1 .. 10 * index + 9`, so plates stack edge to edge. This is an accepted Java
Edition deviation: vanilla Bedrock draws one plate sized to the widest line. A lone line
narrower than two design pixels has no plate. Primitive-shape text uses the same layout.
The billboard rotation uses the original eye-minus-base-anchor direction, while its position
is lifted by `0.125 * (line_count - 1)`. Local X and Y are both negated by the world scale.
There is no text shadow. The accepted open-font deviation remains: our font atlas is converted
using the shared `ui::FONT_DESIGN_PIXEL_TEXELS` instead of duplicating its texel density.

Bold bitmap text draws a second glyph one design pixel to the right. Measurement
adds one design pixel to every positive glyph advance, including spaces; `§r`
resets the style. The shared open-font layout, UI drawing and name-tag atlas use
the same offset and retain shifted ink bounds. Native Unicode drawing uses a
half-pixel offset, as does native TrueType drawing, while measurement still adds one; adapting that branch to the
compiled open font remains incomplete.

Glyph atlas rectangles use exclusive right and bottom bounds for both compiled
and runtime sheet glyphs. CPU rasterization samples only that extent, preserving
every stem texel at native size and scaling; zero-area sources emit no ink.

The current extraction caller appends actor data 84 as a new line only inside the score
distance gate. Our retained below-name objective supplies `score + space + display name`
when the server did not supply actor-data score text; empty synced text remains authoritative.
Render-distance metadata is key 140 on this target (the old tracking table listed a different
version's key). An empty synced player name hides its tag; a missing tag falls back to its spawn
username, without changing its scoreboard name authority.

### Material and shader witnesses

The installed vanilla app defines these in `data/resource_packs/vanilla/materials/ui3D.material`,
with shaders in `data/renderer/materials/Nametag.material.bin` and `UIText.material.bin`.
That install is **1.26.51.01**, so it corroborates the material rules but is not a
version-matched shader-pack witness.

`name_tag` blends with DisableDepthWrite and Always; `name_tag_depth_tested` inherits
the plate and changes to LessEqual. `name_tag_text` blends with Always and does not
disable depth writing. The depth-tested text is named **`name_text_depth_tested`**;
it inherits `sign_text`, enables ALPHA_TEST, and uses LessEqual. The shader discards
sampled glyph alpha below 0.5 **before** multiplying text opacity, so sneak text at
0.125 opacity must survive. Text and background use different compiled shaders.

Matched 1.26.50 pass translation confirms depth writing is material-write AND
material-depth-test-enabled; Always is not DisableDepthTest. The environmental
text dispatcher’s variant 0 preserves depth test/write/comparison, forces RGBA color writes,
then overrides depth bias to **-32, slope 0, clamp 0**. The inherited sign-text bias
10/slope 2 is therefore not the final text state. Our reverse-Z tested text uses the
corresponding +32 constant bias. The primary scene clears depth to 1.0 and passes LessEqual unchanged to BGFX,
using the standard-Z convention. Equal pixel displacement across GPU depth formats
is not established. Pass extraction initializes bias to zero; the plate dispatcher
leaves it zero, preserves material depth state, and clears target-alpha color writes.
Our plate target-alpha masking, explicit
culling/backface variants, and exact native sampler/atlas behavior remain incomplete.

Ordinary SDR name tags now join the encoded-color transparency pass. The atlas
sample is restored to encoded RGB before tinting, and plate opacity stays at 0.25.
This corrects the faint linear-blended background in the inspection build. The
near-version shaders corroborate encoded RGB output; exact current final text
attachment dispatch, HDR and MSAA remain incomplete. A fresh live check is pending.

### Acceptance and incomplete branches

Focused tests cover world size/rotation, multiline background/centering/lift, sneaking alpha,
metadata overrides, score distance, homogeneous projection, safe-area/DPI independence,
texture-page ordering and render graph layering. This is not a closed visual parity gate.

Native material definitions and compiled shaders have now been located as described above;
the prior speculative collision-ray dimming is removed. A matched 1.26.50 shader-pack witness
and native fixed-state comparison are still needed to close exact through-wall/material parity.
Explicit orientation/backface/education/custom-color paths, profanity-filtered names,
vehicle/riding-adjusted anchors, default missing height/render distance, and the inherited
provisional crosshair selection shapes/range remain incomplete. The presentation budget of
128 nearest tags is an upstream client resource bound, not a vanilla visibility rule. Player-default
visibility, current-position/camera-anchor distance details, the retained-score fallback and
NameplateDepthTested flag 129 (outside the current wire flag carrier) remain incomplete.

The canonical debug build was run in Zeqa on 2026-10-01: macOS 26.3, Apple M3 Pro,
Metal, 1280×720 logical viewport, 2560×1440 physical pixels (DPI scale 2). The endpoint
resolved to `40.223.14.30:19132`, advertised protocol 2193 / game 1.26.50 / WaterdogPE
Proxy, and transferred pre-login to `pvp.inpvp.net`. Rendered frames confirm visible
translucent multiline plates, individually centered colored lines, perspective-scaled
text, and first-person/HUD coverage. Native window capture and client F2 frames are
retained privately under `.local/screenshots/`; no game artwork is added to git.
The tested scenarios are in the ignored acceptance record.

Movement and different camera pitches rendered successfully, but the captures do not
form a controlled same-actor near/far comparison: capture focus changes and concurrent
input altered the view, and one approach opened an NPC form. No purchase/form action
was submitted. Controlled anchor drift, sneaking/through-wall depth, near-plane clipping,
and version-matched vanilla fixed-state frame comparisons remain pending. This is a
live rendering smoke pass, not a closed visual parity gate or performance acceptance.
