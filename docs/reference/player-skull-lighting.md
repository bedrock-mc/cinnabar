# Placed skull lighting

The placed-head brightness correction follows vanilla dispatch, material identity
and light-coordinate rules. The installed vanilla client is a near-version
material/shader witness, rather than an identical-version acceptance artifact.
Game and pack targets remain defined by `assets/bedrock-target.json` and
`assets/vanilla-source.json`.

## Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Placed render | Supply the skull’s integer block position to light setup. |
| Light coordinates | Read the cell's light with minimum block light zero. Divide the two retained brightness levels by 16, sample the lightmap at that coordinate, and publish RGB `TILE_LIGHT_COLOR`. |
| Head materials | Ordinary and piglin heads select `mob_head.skinning`; dragon heads select `dragon_head.skinning`. Both inherit the ordinary alpha-tested entity material. |
| Model selection | Use the backing block’s type identity to select model, material and texture, then submit through the block-actor model renderer. Player heads select the player model. |
| Registration | Register `player_head` and its six companion head types as skull blocks. |
| Light sampling | Skull blocks set light filter zero. Read the requested cell’s retained nibbles directly without choosing neighbouring cells. |
| Network light fields | Retain nested byte `lightLevel` for dampening and the distinct nested byte `emission` for emission; serialization uses the same names. |

The installed `1.26.51.01` vanilla material chain is
`mob_head:entity_alphatest`, with point sampling, no culling and an alpha cutoff
of 0.5. Its ordinary Fancy entity shader uses posed world normals, the same
vanilla shading polynomial represented by `render_api::ACTOR_SHADE_COEFFICIENTS`,
and gamma-domain texture/light products. Existing actor lightmap witnesses
establish byte quantization and clamp-linear lookup at the /16 coordinates.

## Zeno custom heads

The affected Zeno Practice lobby heads are server-defined blocks, rather than
vanilla Skull block actors. A read-only authenticated capture from
`zenomc.org:19132` contains 480 custom head definitions. Their components use
`minecraft:light_dampening: {lightLevel: 0}`; their material instances enable
ambient occlusion and face dimming. Nearby live actor tracing found only the
local player and no Skull block-actor NBT at these heads.

The protocol decoder previously looked for a number on the component itself.
It discarded the compound's zero, then the overlay applied its omitted-component
default filter of 15. This incorrectly removed skylight inside every custom
head, making the inset model faces dark. The decoder now retains the nested
network `lightLevel` value for dampening and `emission` for emission. Existing
scalar definitions remain supported; odd non-finite values are skipped. No lighting multiplier,
minimum-light override or material-flag change is involved.

Network-NBT regressions exercise zero dampening and nonzero emission. The
overlay regression verifies that explicit zero survives compilation while an
omitted dampening component still defaults to 15.

The final Rust client build succeeded.
The user tested the rebuilt client on Zeno Practice on macOS/Metal with ordinary
controls and confirmed that the heads render perfectly. All seven explicitly
run Metal shader/readback tests passed. At the user's request, task background
processes were stopped and remaining unit/affected verification was waived
before publishing directly to remote dev. This is visual acceptance of the
reported Zeno issue, not a full version-matched rendering parity gate.

## Vanilla skull correction

Current head identities have one shared mapping in `assets::vanilla_skull_type`.
The compiler gives them an Invisible terrain route: their model belongs to the
block-actor renderer. Description chooses the model from the current block name,
even when `SkullType` is absent or stale; the legacy unsplit `minecraft:skull`
continues to use that NBT field. The old compiler recognized only the unsplit
name, so current head blocks emitted terrain fallback geometry that could cover
the correctly lit block-actor geometry. This is a separate vanilla-head mismatch;
fixing it alone did not resolve Zeno's custom-block light decoding.

Placed skulls retain their block and sky levels in `BlockEntityLight::Actor`.
Both the head and hat emit outward world normals after mounting and yaw,
without terrain face coefficients. The block-entity solid shader uses the
shared actor lightmap, normal shading, gamma transfer and distance fog.
Environment changes update the shared lightmap even when cached geometry is
reused. Other block-entity models retain their scalar lighting path.

The previous scalar `max(block_curve, sky_curve * daylight)` bypassed colored
light, ambient adjustment, brightness and effects, reaching exactly zero in
dark cells. Fixed local terrain coefficients also failed to follow rotated
heads. The correction derives these behaviors from the vanilla material path;
it adds no brightness multiplier or minimum-light override.

Focused mesh regressions check floor/wall normals, both layers, retained light
levels, cache invalidation and isolation from later cracks. Metal readback
tests execute production mesh and shader entry points against numeric
day/night/torch/dark/brightness witnesses, rotations, alpha cutoff and the
retained scalar path. Registry-wide compiler regressions reject terrain models
for current heads, and description regressions cover missing/stale NBT.
Native hat geometry, other block-entity materials and full version-matched
visual parity remain separate gates.
