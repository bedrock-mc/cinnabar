# Vanilla-parity gap tracker

Consolidated 2026-09-28 from a read-only audit against the 26.30 client and
pinned bedrock-samples. Target: version-matched
vanilla Bedrock, except the in-game HUD, which targets Java Edition by owner
decision — chat and scoreboard styling there are intentional and not gaps.
Values marked *(measure)* may now be taken directly from the client references (owner decision).

## Fixed
- Block breaking / interaction: `verified_selection` refused an `Unknown` selected
  slot, blocking all mining before inventory arrived. Now treats it as an empty hand.
- Inventory container routing: re-derived from the vanilla InventoryContent/InventorySlot
  handlers — the dynamic container id keys only generic storage, so a present zero routes
  like absent for every fixed surface (player inv/armor/offhand/cursor). Was dropping the
  player inventory on window 0 / id 29 / dyn Some(0).
- Player skin: loads `.local/assets/skin/player.png` (fallback to generated default),
  uploads it in ClientData instead of the white placeholder, and renders on the local
  body + HUD paperdoll via a synthetic local profile. Minor follow-ups: per-tick skin
  compare could `Arc::ptr_eq` short-circuit; `session.rs` at the 1000-line limit;
  square-only skins; arm_size hardcoded "wide" (slim uploads wide).
- First person: stopped drawing the whole third-person body rig at the camera (it
  occluded the view). Draws no near-camera rig until an arm-only model exists.

## Landed, pending native confirmation
- Movement / anti-cheat "movement cheats": deleted the PlayerAuthInput suppression
  subsystem (send every tick); depenetration resolves per-axis with horizontal clamped
  to intended velocity so an embedded start injects no inputless drift; `pos_delta` is
  the per-tick displacement. Independently reviewed (APPROVE, FIX1 proven red→green).
  Provisional pending native measurement: the retained **vertical** MTV push-out is the
  sole recovery envelope, plus the anchor-probe budgets, the 32-tick history window, and
  the 16-block teleport-snap bound. Owner gate: AC server stops kicking + normal server
  feels unchanged.
- Camera third-person boom: lenient camera collision query skips unknown/unloaded cells
  instead of collapsing to radius 0. Owner: confirm ~4-block boom on servers with custom
  blocks while real walls still shorten it.
- Chat `§k/§l/§o` render (CPU draw-list, not shader — atlas page is per-batch) + translation
  killfeeds resolve. Owner: confirm bold weight / italic shear / obfuscation cadence.

## In progress (branches)
- `task/resource-packs` — server pack download/decrypt/apply (review fixes).
- `task/enhanced-shaders` — opt-in Enhanced render mode (non-parity, off by default).

## JSON-UI engine (clean-room, Bedrock target; Java HUD stays an override)
Owner decision: a faithful 1:1 Bedrock JSON-UI interpreter drives forms, container
screens and menus from the vanilla `ui/*.json` + textures; the Java-styled gameplay HUD
(`hud_screen` family) stays on the existing path and is absent from the engine's screen
allow-list (`json_ui::ENGINE_SCREENS`). Landed (uncompiled in-lane, pending reconcile):
T1-T3; engine-owned widget state (button/toggle/edit-box/slider state children, scroll
views and scrollbar box, slider travel and progress clipping), relative layers, wrapped
labels, grids, hit regions/modal blocking/global mappings, `dropdown_area` re-parenting,
focus auto-scroll, server-pack ui json (with `modifications`) and textures. The optional
`.mcbeui` carrier loads at startup (absent: fallback dialog); action/element/modal/custom
forms always draw through the engine with vanilla input. Container screens draw through
the engine by default (owner decision; the Java-styled screens remain only for a failed
render): survival inventory (paper doll, 2x2 grid, recipe book toggle and panel), crafting
table, chest/large chest/barrel/shulker/ender chest by block entity, every station
(furnace family and brewing progress, anvil name, enchanting options, grindstone, loom
patterns, smithing, cartography, stonecutter recipes, beacon powers, hopper, dispenser,
dropper, crafter, horse), the creative inventory (tabs, search, collapsible groups) and the
two-page book/lectern screen, with full item tooltips, the recipe book filter toggle,
crafter slot toggles/preview/powered arrow, creative's wide list and per-mount equip slots.
A hover change never lays out again; scrolling lays out only visible scroll content.
Layout solves the client's layout-variable rules (sizes and bounds, stack, grid and scroll
components, clipping, locks, anchored and cursor/drag offsets; `crates/json-ui/tests/layout_parity.rs`).
Incomplete: touch scroll dynamics infer the 0.05 s velocity-window blend and no host ticks them;
clip-state events are reported but not dispatched; `size` animations need a host clock.
Incomplete: the enchanting book model, rune font, the live horse renderer, banner
pattern previews, the anvil result preview and repair cost; the paper doll's held item is
a flat quad and it draws no offhand or armor trims. Needs native measurement: the virtual UI scale (engine
pixel = the HUD's GUI pixel), slider travel and `clip_direction`, slider `label: value`
text, tooltip and durability placement/colours, the preview's size and vertical placement in its box, layer
relativity, and the T2 inferences (omitted `size` = 100%, `anchor_to` = parent point). Menus (landed, uncompiled): start, play (worlds/friends/servers tabs), add/edit server,
settings (sections, GUI scale, sound sliders bound to `AudioSettings` at 5% snaps whose
granularity needs measurement; text-to-speech disabled), pause, death, connecting, disconnect reason, device-code sign-in, NPC dialogue
(student view) and server settings (a form over the settings menu, not yet a settings
section) draw from their vanilla screens, as do launcher dialogs (vanilla two-button popup);
profile (OreUI in 26.30, no `ui/*.json` screen) and first-run progress (it completes before the
window opens) stay programmatic, and the programmatic launcher remains only for a missing
carrier or a failed render. Launcher screens sit on the vanilla panorama, a render pass that ray-casts the
carrier's full-resolution cube faces (FOV 85°, 2°/s turn, 25±5° tilt follow the title-screen
cube; the real values remain unresolved and need measurement).
The Servers tab lists featured servers then gatherings with the vanilla info panel (description,
news, screenshots, games; artwork up to 512 px, read-more toggles, selected-row highlight,
RakNet player counts and ping icons with provisional 150/300 ms thresholds); Realms split
owned/member with players and expiry; the start screen shows the profile gamertag and gamerpic
(else the persona head), messaging tile art (first GIF frame), inbox badge, Realms invite
count and the live-event button. OreUI screens that 26.30 shows by default are drawn with our code-drawn OreUI design system
(`docs/oreui.md`): play (Worlds/Realms/Servers), death, profile, inbox and the friends drawer;
settings, disconnect and the main menu stay JSON-UI as in 26.30's defaults. Bed and
create/edit-world OreUI screens are not built. All need screenshot checks. Not
served: persona appearance pieces for the paper doll, gathering venues, store layouts. A launcher run keeps one `-control-status` core (restarted
on sign-in/sign-out) that feeds account, realms, friends and local worlds; joins select their
target over `connect.v1` and fall back to a per-session core; opened local worlds are joined and
closed with their session; respawn sends the client-ready respawn request only (no player
action, per the reference); a dead launcher core is not detected until a join times out.

## Equipment / attachable rendering (Bedrock 3D target)
T0 landed (attachable bindings, `.mcbeeqp` carrier). Uncompiled/unmeasured lane work now adds
(all **incomplete**; no vanilla acceptance gate closes on it):
- **Carrier v2** carries decoded attachable textures (armor tiers, elytra, ...) beside bindings.
- **Layers:** extra actor rig instances keyed `(session, dim, runtime, layer)` ride the body's pose
  and transform; per-instance dye tint word; instance arena 512 (bodies still 128).
- **Held item (third person, both hands):** flat sprite items only, extruded one texel deep and
  packed into shared atlas pages, on `rightItem`/`leftItem`. The display placement is a
  *provisional* placement, not the retail transform (needs native measurement). Block items,
  bow/crossbow/trident geometry, spyglass/horn poses: not drawn.
- **Worn armor:** four slots from `MobArmorEquipment` (remote and local), player-variant
  geometry bound to body bones by name, tier textures, leather dye from `customColor` (default
  leather colour and colour-space multiply need measurement). No enchant glint/trim/elytra/
  shield/pumpkin head.
- **First person:** near-camera rig pass fed with arm-only masking per the pack's first-person
  part visibility (arm shows for empty hand/map only) plus a drawable held sprite or block cube
  in a separate camera-space legacy-icon stack (item atlas bound to the pass); cubes and
  attachables still use the posed `rightItem` bone. The arm follows the 26.30
  reference: a zero-yaw actor in view space, feet one eye height below the camera, with the
  target/body/head rotation queries zeroed; `variable.player_arm_height` is the equip progress
  (its per-tick step and swap height are unresolved in the reference and need measurement).
  The hand pass has its own fixed FOV and receives view bob, hurt tilt and arm sway.
  Undrawable items keep the CPU icon viewmodel. Eat/drink/bow-draw
  poses are neutral: `query.main_hand_item_use_duration` now counts using-item flag ticks, but
  `max_duration` has no source (no item-use state; only food durations exist in pack data).
- **Arm swing:** attacks, mining and use swing the local rig when the swing is accepted (the
  server never echoes the owner's swing); remote swings come from the Animate packet. The pinned
  pack's first-person attack rotation reads `variable.first_person_item_rotation_factor`, which
  neither the pack nor the 26.30 client assigns; it provisionally takes the pack's
  `first_person_rotation_factor`. Haste and fatigue do not yet change the rig's 6-tick swing.
- **Held item placement:** third-person sprites/tools/cubes follow the 26.30 reference's
  held-item/default transforms on `rightItem`. Ordinary first-person icons use the current
  client camera stack and default icon transform.
  Swing/equip inputs are sampled at fixed ticks and interpolated before nonlinear evaluation;
  they do not inherit avatar bones or model scale. Provisional: item-use/mirrored-art branches,
  custom render offsets and native sine-table rounding are missing; the hand-equipped item
  list mirrors vanilla by identifier, the block
  mesh origin is assumed centred, and the narrow-aspect first-person offset is not applied.
  `query.get_default_bone_pivot` now reads the rig's rest pivots.
- **Block items:** plain opaque cubes in hand (third and first person) and on the head
  (carved pumpkin); non-cube blocks and mob/player heads are not drawn.
- **Elytra:** wings posed from the carrier's literal `default`/`sneaking`/`sleeping` clips;
  gliding and swimming are Molang-driven and fall back to `default`.
- **Trident and shield:** single-bone attachable geometry at the hand item bone, placed by the
  carrier's literal wield transforms (shield poses resolved per hand). Attachable origin rule
  (behavior only): the attachable's model origin is the parent's `rightItem`/`leftItem` bone
  origin and bones turn about their own pivots; a geometry with only a `rightitem` locator bone
  (bow, crossbow) draws the extruded item texture there.
- **Worn heads:** skeleton, wither skeleton, zombie, player, creeper heads reuse the
  block-entity carrier's skull textures on the head bone; dragon/piglin heads are not drawn.
- **Item use:** `main_hand_item_max_duration`/`item_remaining_use_duration` read carrier use
  durations (behavior-pack food, spears, honey; ticks) and the local player's held items feed
  the animation runtime. Bow, crossbow, trident, potion, shield and spyglass durations are
  engine-side and unavailable.
- **Not done:** bow/crossbow pull frames (frame index and charge semantics are engine-side),
  spyglass/goat-horn poses, enchant glint (needs an additive pass ordered after the base draw),
  armor trims (per armor x pattern x material composite textures).

## Local player rendering
Third-person body (S1) merged: local player routed through the shared animated rig.
Known gap: a one-tick cosmetic body smear on same-dimension teleport/large snap
(`teleported` is hardcoded false; physics exposes no snap signal yet). First-person
hand (M2) not built (near-camera GPU pass).

First-person hand (near-camera draw node) merged; provisional/native-tunable: the
hand's daylight is pinned to 1.0 (won't darken at night) and its FOV needs native tuning.

Root cause: the local player is never spawned as an actor, so it never gets the
animated rig remotes use. All three below flow from that.
- Third-person body: static 6-bone diagnostic biped placed at camera yaw (no body/head
  split, no walk/idle/swing). Implement: spawn a client-fed local actor, route through
  the shared rig + motion model.
- First-person hand+item: flat CPU sprite / static `EmptyHandNeutralStaticFallback`; no
  bob/swing/equip/sway/lighting/held item. Implement via first-person render controller +
  `animation.player.first_person.*` (public samples) + vanilla's held-item transforms.
- Top-left mini player: no vanilla counterpart → remove if a standalone overlay; keep the
  inventory/menu paperdoll.

## Inventory / containers (Java target)
- Landed uncompiled and unverified: the ledger admits furnace/blast/smoker, brewing, hopper, dispenser/dropper,
  horse, crafter, anvil, enchanting, grindstone, loom, smithing, cartography, stonecutter, beacon and the
  chest family (barrel/shulker names too); named cells address requests by vanilla container name, screen
  inputs by UI slot; `ContainerSetData` drives furnace flame/arrow and brewing bubbles/fuel; enchant options
  (`PlayerEnchantOptions`) select via CraftRecipe. Java-styled screens for each (`presentation/screens`,
  `hud_layout/windows.rs`), creative catalog screen (tabs, search, scroll, take/delete), hover highlight,
  tooltips (name, enchantments, lore), durability bars, server block-entity titles, whole-stack shift-click,
  drag-distribute, double-click gather, craft-all, number-key craft into hotbar, in-world Q drop.
- Provisional *(measure)*: every screen offset, widget shape/colour, shift-click destination order and item
  role tables (`item_roles.rs`), horse/crafter/cartography/smithing-template wire slots, cook totals (200/100)
  and brew total (400), anvil/grindstone/loom/smithing consume amounts and craft-action mix (`screen_actions.rs`).
- Round 2 landed uncompiled: CraftingData screen recipes (stonecutter, cartography, smithing transform and
  trim, multi-recipe ids) feed stonecutter/smithing/cartography result takes; anvil rename field and repair
  multi-recipe id; loom pattern picker (blind take, provisional pattern list); beacon level gating and effect
  icons; ghost slot icons (need item-atlas art named `empty_armor_slot_*`, `empty_armor_slot_shield`,
  `empty_slot_smithing_template`; absent art draws nothing); 26px result cells; Java-styled recipe book
  (craftable recipes, vanilla auto-craft into the cursor, drawn beside the panel instead of shifting it);
  bundle contents (dynamic container 63 keyed by the item's `bundle_id`), tooltip lines, insert/extract;
  writable-book editor, written-book reader and lectern page screens (`BookEdit`, `LecternUpdate`); middle-click
  block pick for survival and creative; right-click with a book in hand opens it.
- Still missing: smithing-trim results (unpredicted), grindstone repair cost and recipe id (no server data
  channel; sent as 0), loom result NBT (client cannot build it), recipe-book search and grid fill, book
  photos, author/xuid on signing, a separate creative pick key, lectern placement while holding a book
  (right-click always opens the book), anvil name prefill, clicks on the recipe-book panel background
  count as outside-panel drops.

## World rendering / atmosphere (Bedrock target)
- Weather precipitation: procedural rain/snow sheets, biome/height classification and per-column surface limits landed uncompiled; optional `make weather-assets` carrier supplies the vanilla rain band and End sky (procedural when absent); biome samples are averaged on a provisional 27-point lattice; additive bolt renderer and flash trigger from `lightning_bolt` actors landed; splash and rain-sound consumers (`RainSplashQueue`, `PrecipitationMix`) are unwired *(measure)*.
- Particles — data-driven engine landed (`crates/render/src/particles`, `make particle-assets`),
  **incomplete, uncompiled and unverified**: block break/crack, LevelEvent and
  SpawnParticleEffect triggers, critical hits. Provisional *(measure)*: break/crack piece
  counts and radii, particle size as half-extent, `particles_alpha` treated as blended
  with a low alpha cutoff, collision-drag model, light mapping, spawn/draw distance caps,
  Molang variable wire layout. Missing:
  local-player eating crumbs (needs the animation lane's item-use state; remote and server
  Feed notices work), fireworks, particle sound playback (queued via
  `ParticleSystem::take_sounds`), actor-bound emitters follow position only, not rotation
  (HIGH until measured).
- Daylight: eased celestial angle, day plateau and night transfer landed uncompiled; the shader night floors
  (`lighting.wgsl`, `chunk/gpu/upload/lighting.rs`, cloud) still clamp at 0.2/0.04 and must drop to `NIGHT_SKY_TRANSFER` (HIGH).
- Stars: procedural star field landed uncompiled; twinkle unverified *(measure)*.
- Leaves: Fancy look landed (leaf↔leaf faces kept); live compare pending.
- Block-entity models: chests (single/double, lid cue), beds, shulkers, banners, skulls (incl. dragon/piglin),
  bell with frame and swing, lectern, enchant-table book, conduit, decorated pot, item/glow frames with framed items
  (dropped-item sprite path), campfire items, tinted scrolling beacon beam, additive end portal, sign text and
  model-shaped crack overlay draw from the `.mcbeben` carrier; all uncompiled, unmeasured and lit by the terrain
  light curve. Filled maps, spawner mob, flower-pot plants, conduit wind cube, campfire/hopper/brewing-stand
  terrain models and exact bell/lectern/pot dimensions remain open (MED-HIGH).
- Server resource packs: custom blocks (sequential and hashed ids), item icons, and lang apply at runtime;
  custom entities apply in the neutral material profile only (pack attachables apply to held/worn items on player bodies through a per-session equipment layer; no custom materials, cross-catalog vanilla clip references, or conditional/multi-texture render controllers); pack property defaults seed query.property only when a resource pack carries `entities/` behavior definitions; vanilla-entity retexturing rides the same path when the pack redefines a vanilla identifier; merged pack sounds now feed the audio engine as server overrides (uncompiled) (HIGH).
- Sky now biome-temperature-derived, fog linear and rain-blended; clouds uncalibrated, End sky from the optional carrier,
  sun/moon quad size, AO darkening step, water surface alpha *(measure)*.
- Terrain blocks (uncompiled): ice, slime, honey, tinted glass, powder snow, snow layers, named opaque cubes, amethyst
  and standing coral-fan sprites, redstone bases now compile; lanterns, candles, end/lightning rods, cauldron, hopper,
  anvils, pistons, scaffolding, dripleaf, campfire and slime/honey inner cubes still diagnostic pending measurement (HIGH).

## HUD (Java target; chat/scoreboard intentionally Java — not gaps)
- Title/subtitle/action bar centered, magnified, alpha-faded from SetTitle timings; placement constants need measurement (uncompiled).
- Screen overlays: see the camera section (dedicated overlay pass landed uncompiled; underwater overlay not listed there) (MED). No red damage flash is correct.
- Boss-bar colors/notches approximate; effect-blink approximate; boss-bar Java sprites (no source pack carries them; notches stay procedural) (LOW). Hardcore hearts ship via the optional `make hud-extras-assets` carrier. Heart jitter/regen wave, hunger shake, boxed sliding toasts remain unaccepted. Name tags now use the native world-plane geometry, multiline background and independent centering; [source record and remaining branches](../reference/nametag-rendering.md). No native visual gate is closed. Offhand handedness has no Bedrock source.
- Chat/killfeed glyphs: ranges widened (IPA/small caps, super/subscripts, number forms) and zero-width/control/variation-selector code points now lay out as nothing; needs `make assets` and live recheck of the garbling (MED).
- Round 3, all uncompiled: AvailableCommands drives chat suggestions (names, enums, soft enums, targets, usage hint, permission filter, Tab cycling; Enter always sends); F2 screenshot to `screenshots/` with chat confirmation (UTC names); bed screen (sleep tint, Leave Bed, StopSleeping; tint timing/colour and button geometry need measurement); F3 debug overlay (targeted block shows runtime id only; no block-name lookup).
- Faithful already: hotbar, hearts/armor/absorption, hunger, air, XP, crosshair.

## Camera / view (Bedrock target)
- Third-person boom collapses onto the player (camera reads as "too close"): the collision
  avoidance fails closed to radius 0 when the sweep errors or hits geometry; boom radius 4.0
  is itself vanilla-correct. Model height is correct — this is distance only (MED, confirmed live).
- Dynamic FOV multiplier follows vanilla (movement-speed ratio, slowness, flying, bow, spyglass 0.1, tick smoothing);
  swim-speed factor, underwater narrowing and the final [5, 130] clamp are missing. Walk view-bob,
  hurt tilt, nausea/portal wobble, server shake and `CameraInstruction` set/clear/fade/FOV are implemented
  presentation-only under `app/src/camera/`; their magnitudes, curves and signs are provisional *(measure)* (MED).
- Screen overlays (pumpkin blur, spyglass scope, portal, freezing, suffocation, fire, server fade) draw in a dedicated
  pass (`ScreenOverlayRenderPlugin`); pumpkin and spyglass use the vanilla PNGs when found and procedural art
  otherwise; portal, fire and freezing are procedural, suffocation is a flat tint, and the vignette stays with the
  HUD renderer (MED).
- Presets, target, attach and detach are applied; orbit presets ignore collision. Not applied: view/entity offsets,
  spline instructions, blindness/darkness/night-vision consumers (`VisionEffects`), first-person hand consumer of
  `FirstPersonHandMotion` (MED).
- Look sensitivity now follows a provisional slider curve, gamepad look is frame-rate normalized, optional
  cinematic smoothing; pitch clamp 89.9 vs 90 still *(measure)*; FOV default 60 and range 30..110 match vanilla (MED).

## Movement / physics / controls (Bedrock target)
Core physics binary-confirmed correct (gravity/drag/friction/jump/speed). Gaps:
- Live movement still `FreeCamera`; validated physics not yet the production source (known).
- Sprint latch (key, double-tap, toggle option) and toggle-sneak are implemented but provisional
  (double-tap window unmeasured); item-use sprint stop is not classified yet.
- Ability flight, pose-swimming, elytra gliding and crawl/forced-sneak are provisional simulator
  modes (no oracle; coefficients need measurement); firework boost relies on server motion only.
- Scaffolding is solid only from above and sneak descends (provisional rate). Honey jump/slide,
  soul speed and depth strider are simulated with provisional coefficients; sweet berry bush
  slowdown, dolphin's grace and client-predicted vehicles are open.
- Riding: player physics is suspended while mounted, the rider follows its mount seat, and boat
  paddle flags are sent; client-predicted vehicles (no vehicle simulator) and
  the vehicle predicate/coefficients (see plan.md) are open (HIGH); `ClientMovementPredictionSync` is sent after corrections (interval provisional). Sweet berry bush needs the
  registry regeneration described in plan.md.
- Step height 0.6 vs ~0.5625 *(measure; oracle-validated value left unchanged)*; lava strata;
  scroll-notch magnitude; UI key-repeat (MED/LOW).

## Audio
- Not yet audited. Earlier note: no footstep/block-sound lookups by runtime id exist — likely a large gap.

## Entity animation query audit (Bedrock target)

Queries referenced by the vanilla pack's entity, controller, animation and render-controller JSON
(use count in parentheses), by how Cinnabar evaluates them. Sources: `actor_animation/query.rs`,
`tick.rs`. Unlisted queries read 0.

| Status | Queries |
| --- | --- |
| Metadata flag word | is_sneaking, is_sprinting, is_swimming, is_gliding, is_crawling, is_baby (293), is_saddled, is_chested, is_powered, is_tamed, is_sitting, is_angry, is_charging, is_casting, is_eating, is_emoting, is_using_item, is_delayed_attacking, blocking, is_dancing, is_standing, is_playing_dead, plus the remaining `is_*` behaviour flags |
| Metadata value | variant (69), mark_variant, skin_id, model_scale, sit_amount, lie_amount, fuse_time, invulnerable_ticks, swelling_dir, swell_amount (normaliser unmeasured), has_target, get_name |
| Client-derived motion | modified_move_speed (269), modified_distance_moved (226), walk_distance, ground_speed, vertical_speed, position_delta, movement_direction, is_moving, is_on_ground, life_time, anim_time, delta_time, time_stamp (tick count, not world clock), body/head/target x/y rotation |
| Links and equipment | is_riding, has_rider, has_player_rider, is_riding_any_entity_of_type, get_equipped_item_name, is_item_equipped, is_item_name_any, is_sleeping |
| Status / attributes | health, is_alive, hurt_time, hurt_direction, death_ticks, is_shield_powered |
| Item use | main_hand_item_use_duration (ticks the use flag has been set, in seconds) |
| Block sample | is_in_water, is_in_lava (block at the actor's feet, app-fed each frame; is_in_water falls back to the swimming flag or airborne fish before the first sample) |
| Smoothed | swim_amount (ramps toward the swimming flag; step unmeasured) |
| Heuristic | is_grazing (eating flag, unmeasured), standing_scale (unsmoothed 0/1) |
| World / item state | sleep_rotation (bed `direction` state under the sleeper, quarter turns; origin unmeasured), item_is_charged (crossbow `chargedItem` NBT kept on the canonical stack), has_cape (skin carries a valid cape image), property (SyncActorProperty names resolved per entity type; enums read as their value name) |
| Armor | armor_texture_slot, armor_color_slot (equipment store; chainmail, turtle, elytra indices unmeasured) |
| Idle (0) | main_hand_item_max_duration, item_remaining_use_duration, has_head_gear, is_spectator, frame_alpha (evaluated at tick boundaries by design), armor_material_slot, equipped_item_any_tag, kinetic_weapon_*, bone_*/get_root_locator_offset, surface_particle_*, panda counters, wing/tail/shake values, is_levitating, is_jumping |

Engine-fed variables: attack_time, gliding_speed_value, is_holding_right/left, is_sneaking,
is_blocking, damage_nearby_mobs, is_first_person, player_x_rotation, bob_animation, swim_amount,
left/right_arm_swim_amount, has_target (per tick); the rest of the seeded set in `evaluation.rs`
(charge_amount, arm offsets) stays at its seed.

Local player: sneak and sprint come from the latest predicted tick, swim is sprint while in
water, and using follows the accepted air-use lifecycle in `item_use.rs` (bow, trident,
spyglass, crossbow: click-air on the press, release on button-up, completion when a crossbow's
charge runs out). Blocking is always the server flag. Incomplete: other use items (food, drink,
throwables, spears) send no click-air, trident durability/Riptide admission and the item-use
movement slowdown are unmodeled, and the Quick Charge completion tick is unmeasured. Glide, crawl and sleep arrive from server metadata; the
movement simulator models none of them, and sleep_rotation samples the bed under the local rig.

Riders are placed at mount position plus a seat offset rotated by the mount's yaw each tick: the
streamed offset (metadata key 56) when present, else the mount type's `minecraft:rideable` seat
from the local behavior pack (chosen by rider count and unique-id order; absent pack means no
defaults); layouts are picked by the mount's saddled, baby, tamed and sheared flags, and the
seat's `rotate_rider_by` (numeric only) and `lock_rider_rotation` turn the rider's body with the
mount and clamp its head. The offset frame, vertical origin, seat ordering and rotation locks need native
verification. Invisible bodies draw as NoDraw after equipment layers are built, so armor and held
items stay.

Open: `armor_material_slot` semantics are unmeasured.

### Actor metadata keys (protocol 1001, gophertunnel `entity_metadata.go`)

Presentation-relevant keys and how the client consumes them; keys absent here (commands, trades,
aim assist, sounds, buoyancy, container data) have no visual effect and are retained unread.

| Key | Consumed | Vanilla effect |
| --- | --- | --- |
| 0 / 92 flags | yes | every `is_*` query, on-fire camera overlay, invisible body (NoDraw; armor and held items stay, as vanilla never hides held items for invisibility), show/always-show name, sneak tag dimming, sleeping, riding layouts (saddled, baby, tamed, sheared) |
| 1 structural_integrity, 2 variant, 43 mark_variant, 104 skin_id, 101 trade_tier, 48 invulnerable_ticks, 55 fuse_time, 21 swell_dir | yes | integer queries; render-controller texture, geometry and part-visibility arrays re-evaluated per tick for vanilla and server-pack entities alike |
| 3 color, 82 color2 | no | engine-side dye tint (sheep wool, shulker, tropical fish, llama carpet); not a Molang query — missing |
| 4 name, 81 always_show_nametag, 84 score_tag, 143 nameplate_render_distance_max | yes | nametag text (players included), forced visibility, score line within 10 blocks, tag range (default 64) |
| 38 scale | yes | multiplies the authored model scale for body, equipment, texture layers and culling; `query.model_scale`; default nametag height. Hitbox and published nametag height come from 53/54, which the server scales. The first-person hand keeps the authored scale |
| 53 width, 54 height | yes | hitbox (melee, block use, particles), nametag height, primed-TNT offset |
| 56 seat_offset | yes | rider placement; whether the mount's scale scales authored seats is unverified |
| 5 owner, 6 target, 12 hurt_direction, 15 value, 16 display_block, 19 swell, 23 carry_block, 26 player_flags, 37 leash_holder, 89 sit_amount, 93 lie_amount | yes | ownership/leash ropes, look-at, hurt tilt, XP orb frame, minecart block, creeper swell, enderman block query, sleeping, pose blends |
| 7 air, 42 max_air, 120 freezing | yes | HUD bubbles and freeze vignette (local player) |
| 84 score, 140 nameplate_render_distance_max (current target) | yes | synced below-name score inside the native ten-block gate; per-actor tag range |
| 136 filtered_name (older table's target) | no | filtered tag text — missing; current target key mapping still needs reconciliation |

Missing presentation that the flags drive: the entity flame billboard
and `on_fire_color`, entity ground shadows (none are drawn at any scale), the charged-creeper
armor layer (needs `uv_anim`), and per-tick Molang `scripts.scale` (only a constant authored scale
is carried).

## Entity render controllers (Bedrock target)

Incomplete: the neutral single-texture artwork admission is gone; every rig with a decodable
texture now gets artwork. The entity carrier holds each rig's render layers (per controller:
activation, texture candidates, part visibility, colours), evaluated per tick against the actor's
Molang state, and every texture a candidate can select is built into the actor pages.

| Status | Coverage |
| --- | --- |
| Drawn | geometry candidates from ternary and `Array.x[expr]` expressions, re-selected every tick (baby, sheared), texture aliases, `Array.x[expr]` selection, nested ternaries, several `textures` entries per controller (drawn as stacked layers), several controllers per entity that share the rig's geometry, `part_visibility` (bone-name patterns, trailing `*`), `color` as the tint, `overlay_color` |
| Approximated | materials are all drawn with the neutral binary-alpha material; a controller layer whose `geometry` never selects the rig's default geometry is skipped; hidden bones hide only their own cubes; fractional-alpha and mis-sized variant rasters are omitted; an entity without a `default` geometry alias rests on its first alias until its controller selects |
| Missing | `uv_anim`, `light_color_multiplier`, `ignore_lighting`, `is_hurt_color`, `on_fire_color` (compiled, not drawn), per-bone `materials`, controllers using another geometry (sheep wool geometry, cape-style second rigs other than the player cape) |

Player skins: a skin's own geometry (resource patch `geometry.default`, inheritance only within
the skin's JSON, lenient field parsing) replaces the default humanoid, driven by the player's
animations through matching bone names; equipment rides it by bone name. Provisional: the skin
page is 64x64, so 128x128 images are downsampled; `animation_data` (animated face/textures) and
the patch's other geometry, animation and flag keys are ignored; persona skins draw the sender's
baked image instead of vanilla's piece rebuild; an unloadable image or unresolved model falls back
to the pack's Steve skin and default humanoid (vanilla uses its skin pack's Dummy skin, absent from
the samples). Legacy 64x32 images are expanded as vanilla does.

Player cape: drawn from the skin's cape raster with the `geometry.cape` mesh posed from the
player's bones by name; the cape's rest turn, layer resampling and `cape_flap_amount` scale need
native parity verification. The entity compiler retains vanilla's `geometry.cape` from
`models/mobs.json` in the geometry carrier and reference sidecar. Server-assigned local appearances
survive unchanged client pose feeds and roster removal while the player actor remains alive.
