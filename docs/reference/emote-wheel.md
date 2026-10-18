# Emote wheel and catalog playback

Cinnabar opens the native JSON-UI wheel with the remappable `key.emote` control.
The defaults are keyboard B and controller D-pad Left. Digits 1–4 choose the top,
right, bottom and left slots; the pointer uses the radial control's dead zone.
Change Emotes lists supported owned clips in pages of four. Choose a clip, then
its destination slot. Equipping an equipped clip moves or swaps it.

C or controller Y opens Change Emotes. Controller shortcuts honor the configured
X/Y and A/B swaps. Tab, controller RB, or Next page advances the catalog.
Fresh profiles equip owned clips in catalog order, up to four slots. An empty
catalog leaves empty slots. Saved slot IDs resolve against the current catalog;
removed or unowned clips cannot play. Catalog rollbacks activate the exact
admitted revision, including its playback data and ownership metadata.

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

## Catalog clips

The API repository owns all emote choreography, including Twerk and Breakdance,
and their reproducible exports. The client contains generic clip playback and
articulation only. Its optional catalog `emotes` array provides each clip's ID,
display name, free ownership and content-addressed asset. Free clips are owned
by every account. Unsupported and unowned entries stay out of Change Emotes.

The cosmetics worker downloads same-origin assets within bounded byte counts
and verifies SHA-256 on download and every cache read. Invalid clips are skipped
and counted. Previously downloaded clips remain available offline.

The pinned API schema defines version 1 clips: duration, loop metadata and local
joint tracks with quaternion XYZW rotations and bind-relative offsets in leg-height
units. The rig and budgets come from that schema. Rotations use shortest-path
normalized linear interpolation; offsets interpolate linearly. Single keys are
constant; longer tracks span the clip duration with strictly increasing times.
Non-looping clips hold their final pose.

Classic cuboid skins receive render-only elbows, wrists, knees, ankles and a
lower torso. Cropped UVs, clothing, face layers and held-item anchors follow the
joints. All clips share the prepared rig. Contact correction uses visible body
pieces and is shared with attachments, while preserving authored vertical root
motion for hops and jumps. Unsupported authored models keep their original mesh.
Unusual proportions and native/Marketplace animation parity remain incomplete.

Sampling resolves named bones and composes body and attached layers together.
Render time advances playback independently of simulation tick speed. The native
first-person hand and actors without shared playback retain their normal poses.
Gameplay actions, other modal screens and actor/session resets cancel playback.
Perspective remains user controlled.

Starting or restarting a clip reports its catalog ID and phase through the
Cinnabar API. Custom playback does not set Bedrock's native Emoting flag or send
custom IDs over the Bedrock emote packet channel. Nearby Cinnabar players receive
the shared overlay when both clients report each other nearby. Visibility settings
control publishing. Stops, world changes, actor-lifetime changes and expired API
leases retire the overlay.

## Coverage and remaining work

Synthetic fixtures cover clip validation, revision rollback, owned catalog defaults,
saved slots, paging, controller mapping, cancellation and native pose isolation.
Animation tests cover articulated geometry, texture cropping, clothing, face layers,
held-item anchors, visible contacts and authored root lift at both pose endpoints.
The API checks reproducible exports and catalog asset hashes and serves each clip
through its asset endpoint. Production catalog clips stay out of client fixtures.

Native Dressing Room navigation, Marketplace ownership/playback, analog-stick and
touch navigation, and exact native animation parity remain incomplete.
