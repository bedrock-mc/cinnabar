# Emote wheel and custom playback

Cinnabar opens the native JSON-UI wheel with the remappable `key.emote` control.
The defaults are keyboard B and controller D-pad Left. Digits 1–4 choose its top, right, bottom and left
slots; pointer selection uses the authored radial control's geometry and dead zone.
The control is listed in Keyboard & Mouse and Controller settings. Change Emotes equips the
original custom dance in a chosen slot and persists the complete slot array.
Equipping an already equipped piece moves/swaps it rather than duplicating it.

## Vanilla rules

- The emote action is named `key.emote`; its keyboard default is B.
- The pinned pack's `persona_emote.emote_wheel_screen` and
  `persona_common.emote_wheel_panel` define four cardinal slots. The
  controller's contextual global bindings query each originating control's
  `#index`.
- The controller default is D-pad Left, bound as `button.emote`. Supplemental
  binding rows are appended to preserve saved IDs.
- Playing an emote slot accepts slots 0–3. Equipping an emote bounds the slot
  and swaps an already equipped piece into the requested slot.
- The selection wheel's radius follows the shorter rectangle side; the inner
  boundary is excluded and the outer is included. Visibility selects one state,
  and the wheel starts with no hovered slice.
- Playing an emote drives animation controller playback and emoting status; it
  does not directly change the camera perspective.
- Nonzero horizontal movement requests emote cancellation.
- Emoting keeps the HUD paper doll visible with the existing hold timer.

## Original custom emote

Twerk is an owned animation authored from observing the public
[Lunar product preview](https://store.lunarclient.com/products/3720241/twerk).
The listing identifies a repeating dance that stops on movement. The shared
custom catalog owns its loop period, identifier and label; no Lunar animation
files or Marketplace entitlement identifiers are included.

The revised owned clip follows the public video's sustained deep squat, wide
stance, level head, hands beside the thighs and hip pulse. Model-space joint
targets rotate the torso about the hips even on independent, shoulder-pivot
skin bones; clothing still follows its actual parent hierarchy. Root Y/Z offsets
anchor the leg bottom-face centers through the loop. Torso counter-tilt keeps
shoulder/head height steady as the pelvis rocks; varying spread keeps the foot
centers fixed laterally as well. Rigid tilted foot corners
are not articulated soles, and exact visual parity remains incomplete.

The user rejected the initial steady-head revision as still reading as a crouch.
The next owned revision doubles the leg-driven hip excursion, makes the torso
more upright, adds a small alternating torso twist, and brings the hands toward
the thighs. Foot centers and head height remain anchored. This is still an
approximation on the existing rigid limb model, not verified Lunar keyframes
or an articulated knee rig. The subsequent knee revision below supersedes this
rigid-leg approximation for supported skins.

Classic cuboid skins now receive a temporary render-only thigh/shin split during
playback. Side-face UVs are cropped at the knee rather than stretching or repeating
the whole leg texture; mirrored legs and clothing retain their source mappings.
A two-segment solve bends the knees forward while keeping foot centers planted.
The pelvis pulses beneath steady shoulders. Source models, tick-owned poses,
first-person hands and remote actors keep their original skeletons; stopping the
emote restores the original mesh and pose. Unsupported rotated/polygon/custom
limbs retain the rigid approximation. Rigid armor knee articulation and exact
Lunar animation parity remain incomplete. The user accepted the installed knee
revision. A follow-up requested faster playback and more vertical
pelvis movement. The shared catalog now selects a shorter loop, and the planted
knee solve permits a larger vertical hip pulse while retaining steady shoulders.
This follow-up awaits user verification.
The foot-planting follow-up adds temporary ankle joints and solves thigh/shin
lengths to a fixed ankle position. Each foot keeps its rest orientation, so all
four sole corners stay fixed on the floor throughout the faster vertical pulse.
Foot and clothing UVs are cropped at the ankle; ordinary playback retirement
still restores the original skeleton. The whole-sole regression runs every loop
phase, including both vertical hip extremes.

Sampling resolves named bones in the player's actual geometry and applies
channels before skeleton composition. Body, clothing/persona layers and armor
share that pose. The native first-person hand and other actors keep their normal
animation. Render time drives the loop independently of simulation tick speed.
Movement, jump, attack/use, other modal screens, loss of focus, and actor/session
reset cancel local playback. F5 perspective remains user controlled.

Starting a selected clip queues an EmotePacket in the selection frame, using the
catalog's own identifier and duration. Opening the wheel, cancelling it, and idle
frames send no start packet. Restarting a slot announces a new start even when
the render clock has not advanced. The original custom clip keeps its own
identifier; standard Bedrock clients do not have that clip. Exact Lunar keyframes,
native Marketplace ownership/playback, and remote custom-emote synchronization
remain incomplete. The small custom equip flow substitutes for native Dressing
Room navigation and does not close that parity gate.
Analog-stick and touch wheel navigation remain incomplete; controller navigation
currently uses the D-pad and confirm/back buttons.

## Verification checkpoint

The complete JSON-UI suite passes (480 tests), including native wheel state and
indexed bindings plus square-panel sizing at different aspect ratios. Focused
tests cover saved slots/bindings, skeleton/clothing sampling, native wheel/equip
rendering and radial hits, GUI player vertices, current-frame cancellation,
same-frame input consumption, controller opening, session/focus retirement,
and production actor publication with native hand/remote ownership preserved.
Eleven custom-animation tests pass, including native joint landmarks, foot-center
anchoring, attached arms/head and equivalent flat-biped/clothing retargeting.
The twist regression also checks level head ownership and planted feet.
Knee regressions check connected thigh/shin segments, planted feet,
steady head height, texture cropping, mirroring, clothing and source immutability.
The user accepted the faster hip pulse but reported a sliced-looking ankle seam.
The local correction keeps the ankle inside overlapping textured foot/shin
volumes, with a full-loop overlap regression and unchanged sole corners.
The user accepted its installed Windows appearance.
Controller regressions also cover equipping/playing the left slot without
retriggering the wheel opener, and closing-frame inventory consumption followed
by fresh real drop/book actions.
Software-rendered 1280x720 DPI-1 wheel/equip frames were inspected for readable
text, complete geometry and clipping. This does not replace the required live
Windows game pass; that acceptance is pending and the feature remains local.
Strict all-target clippy passes for all six changed packages. The required
`verify-affected --base origin/dev` run passes formatting, architecture and
compilation, then stops at the unchanged meshing regression
`compiled_ice_over_water_preserves_still_source_and_flat_top` in
`crates/meshing/tests/liquid_barriers.rs` (the touching liquid-face assertion).
That failure also occurred before this feature; emote work does not modify meshing.
