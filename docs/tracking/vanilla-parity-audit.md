# Vanilla parity audit backlog

Audited baseline: `376ff547967cadd8922c22480993d9e64bfe7856` on 2026-08-20.

This is the durable repository-wide mismatch inventory. It complements `plan.md`; it does not
close any implementation, native, visual, timing, performance, or review gate. Gameplay HUD
appearance follows the Java Edition exception in `AGENTS.md`. Packets, world behavior, menus,
inventory, controls, movement, actors, viewmodels, audio, and reconciliation remain Bedrock
authority surfaces unless that exception says otherwise.

Classification:

- **Proven mismatch**: current code directly contradicts the stated product contract or drops a
  required state/feature.
- **Open contract**: current behavior is an explicit policy or approximation that still needs an
  exact-version native witness before it can be changed or accepted.
- **Missing surface**: the required runtime path does not exist.
- **Deliberate deviation**: owner-approved bring-up behavior that must remain labeled incomplete.
- **Implementation landed**: the bounded code fix is locally integrated and reviewed, while any
  wider native/live or surrounding parity gate remains open.

## Landed audit tranches

| IDs | Reviewed task head | Integrated commits | Evidence state |
| --- | --- | --- | --- |
| VPA-004 | `469fe1bb3bfcab865bb6bb2e468ebb4df5d9414d` | `e21e99b6` | App-owned verified cache and semantic-skip isolation; coordinator protocol/client suites, formatting, strict Clippy, architecture, and fresh Sol-high review passed. Local only, not pushed. |
| VPA-008 | `582ac0a099123a61588fca90e9d55648a23e99ae` | `a63431b0`, `29e6baaa`, `6c7e2672` | Verified/predicted stack encoding, latest-wins retry, session reset, and server-forced cancellation; coordinator protocol/client suites, formatting, strict Clippy, architecture, and fresh Sol-high re-review passed. Local only, not pushed. |
| VPA-005, VPA-006 | `a9f24e96ef1a395acc2f0cdbb5f48b4f6f0d4a12` | `76f8c87c` | Failed request columns stay outside loaded/cohort readiness, and request-mode LevelChunks join the required cohort only after decode and admission; coordinator client-world suites, formatting, strict Clippy, architecture, and fresh Sol-high review passed. Local only, not pushed. |
| VPA-106 | `eed6a4fc8b14e5a57fdb0cce3dab22bbf56f60f5` | `9dfecb80` | Non-finite remote player poses and unknown equipment containers are skipped at the semantic boundary without ending the session or overwriting usable actor state; coordinator protocol suites, formatting, strict Clippy, architecture, and fresh Sol-high review passed. Local only, not pushed. |

## P0 — session, state, and player-visible blockers

| ID | Classification | Current mismatch | Primary evidence | Smallest independent closure |
| --- | --- | --- | --- | --- |
| VPA-001 | Proven mismatch | Protocol 2168 runs against protocol-1001 world, light, physics, visual, and coverage carriers. | `app/src/asset_startup.rs`, `app/src/app.rs`, `tools/visualcoverage`, `plan.md` | Build and atomically bind one version-coherent 2168 carrier set; reject mixed sets before gameplay. |
| VPA-002 | Deliberate deviation | Server-required packs are downgraded on the private hop; selected packs are retained but never extracted, merged, decrypted, applied, or presented. | `core/proxy/resource_pack_admission.go`, `crates/resource-pack`, `app/src/runtime/network` | Truthfully reject required packs until bounded stack application and revision-safe renderer/UI swap exist. |
| VPA-003 | Missing surface | Post-login `Transfer` is forwarded/ignored instead of ending the old game session and starting a bounded replacement. | `core/proxy/proxy.go`, `crates/protocol/src/login.rs` | Add a transfer event, old-session teardown, cache/reset boundary, target validation, and replacement handoff. |
| VPA-004 | Implementation landed | Verified blob entries now survive app network-worker replacement through one process-owned cache; unrelated semantic skips leave pending cached terrain intact. | `app/src/app.rs`, `app/src/menu/input.rs`, `crates/protocol/src/login.rs` | Preserve this boundary while the separate cache ordering, miss timing, and persistent-storage gates remain open. |
| VPA-005 | Implementation landed | Request-mode failures drain their bounded transport state but no longer mark the collision-incomplete column loaded or complete its required cohort. | `crates/client-world/src/stream/retries.rs`, `cohort.rs`, `sequencing.rs` | Preserve this boundary while exact retention, timeout, and retry policies remain open. |
| VPA-006 | Implementation landed | A request-mode `LevelChunk` now joins the required cohort only after successful decode, active-column admission, and supported-dimension admission. | `crates/client-world/src/stream/sequencing.rs` | Preserve this ordering while exact recovery behavior for rejected announcements remains open. |
| VPA-007 | Proven mismatch | Block-item routes are omitted from the icon carrier; many sprite items are keyed by atlas aliases while UI lookup uses network item identifiers. | `crates/asset-compiler/src/entity/item.rs`, `icon.rs`, `app/src/ui_runtime/presentation.rs` | Add an authority-backed item identity to icon/model route, including rendered block-item icons and family variants. |
| VPA-008 | Implementation landed | Hotbar selection sends the checked current ledger prediction, preserves unknown/empty/present, retries a full channel, and cancels stale pending sends on a valid server-forced selection. | `crates/protocol/src/item.rs`, `app/src/hotbar.rs`, `app/src/ui_runtime/inventory_ledger.rs` | Preserve this boundary while selected-store unification and native/live equipment acceptance remain open. |
| VPA-009 | Proven mismatch | Selected-stack authority is split between the HUD mirror and inventory ledger, so prediction can disagree across inventory, hotbar, viewmodel, equipment, and packets. | `app/src/ui_runtime/gameplay_authority.rs`, `gameplay_hud.rs`, `inventory_ledger.rs` | Make one selected snapshot authority feed every consumer and test accepted/rejected pending mutations. |
| VPA-010 | Proven mismatch | Raw, analogue, and processed `PlayerAuthInput` vectors collapse to the same processed sample. | `crates/input/src/router.rs`, `app/src/movement.rs` | Preserve device/raw axes and independently measured processing stages through packet construction. |
| VPA-011 | Proven mismatch | Jump/sneak/sprint flags are derived from held input rather than processed transitions; swim, climb, flight, item-use, and related states are absent. | `app/src/movement/encoding.rs`, `app/src/movement/physics.rs` | Introduce processed movement state and exact per-mode flag witnesses before changing the encoder. |
| VPA-012 | Proven mismatch | Sneak/swim/crawl/flight have no shared pose/mode authority; body box, eye height, play mode, gravity, and collision stay standing/normal. | `crates/sim/src/aabb.rs`, `app/src/movement/physics.rs`, `crates/protocol/src/movement.rs` | Add one pose/mode state shared by simulation, camera, rendering, and packet production. |
| VPA-013 | Proven mismatch | Water uses broad intersection, fixed jump/drag, fixed eye height, and no swim/surface-ascent state; holding jump at the surface cannot reproduce the target client. | `crates/sim/src/simulator.rs`, `crates/sim/src/simulator/environment.rs`, `app/src/movement/physics.rs` | Add measured immersion, surface, pitch/swim control, transition timing, pose, and camera witnesses. |
| VPA-014 | Proven mismatch | Production movement consumes the old physics registry, so collision, fluid, friction, climb, and surface facts can disagree with decoded states. | `app/src/app.rs`, `app/src/movement/physics.rs` | Version-tag and fail closed on physics/carrier mismatch; generate current facts before enabling movement. |
| VPA-015 | Missing surface | Dropped-item packets, several actor-motion/event families, remote armor, and retained hand equipment do not reach world rendering. Non-player actors are no-draw. | `crates/protocol/src/login.rs`, `client-world/actor_store`, `app/src/presentation/actors.rs` | Add one packet/lifetime/render family at a time with two-client and native gallery gates. |
| VPA-016 | Proven mismatch | First-person held items are UI-raster approximations with identity transforms and dormant bob/action state, not the renderer-owned Bedrock viewmodel. | `app/src/ui_runtime/presentation/item_viewmodel.rs`, `viewmodel_bob.rs`, `crates/asset-compiler/src/entity/item.rs` | Route the selected item through renderer-owned sprite/block geometry, display transforms, and action timelines. |
| VPA-017 | Missing surface | Audio events are normalized and drained into an unconsumed message; there is no resolver, mixer, playback, positional state, looping, or stop handle. | `crates/protocol/src/audio.rs`, `app/src/runtime/world.rs`, `app/src/app.rs` | Bind the verified catalog and implement one named positional play/stop vertical slice before wider audio families. |
| VPA-018 | Proven mismatch | World and atmosphere carriers can have structurally valid but stale/wrong source identity and still reach startup. | `crates/assets/src/compiled.rs`, `app/src/asset_startup.rs` | Put protocol/source/registry identity in every required carrier and test mixed/stale substitution failures. |

## P1 — correctness and lifecycle gaps

| ID | Classification | Current mismatch | Primary evidence | Smallest independent closure |
| --- | --- | --- | --- | --- |
| VPA-101 | Open contract | Cache publication/status/recovery ordering is per-column and permits unrelated work to overtake unresolved cached terrain; exact retail scope is unproven. | `crates/protocol/src/blob_cache/resolver.rs`, `crates/protocol/tests/blob_cache` | Capture concurrent-column misses plus actor/movement/radius traffic; then encode the observed dependency scope. |
| VPA-102 | Open contract | Empty cache-miss responses retain transactions indefinitely; cross-transaction missing-hash suppression is an implementation policy. | `crates/protocol/src/blob_cache/resolver.rs`, `crates/protocol/tests/blob_cache` | Establish miss timeout/retry/duplicate behavior with an exact-version transcript. |
| VPA-103 | Proven mismatch | Semantically unrelated world rejection can be confused with malformed inner chunk/equipment wire; some truncation is skipped rather than fatal. | `crates/protocol/src/login.rs`, `crates/world/src/error.rs` | Preserve wire-fatal provenance through inner payload decoding; keep semantic oddities counted and survivable. |
| VPA-104 | Open contract | Radius/publisher values are clamped to 16 and request packets are one-column/128-entry policy, while the wire and client paths allow broader geometry. | `crates/client-world/src/stream`, `crates/protocol/src/world/requests.rs` | Measure radius above 16, request batching, publisher movement, and edge retention before lifting limits. |
| VPA-105 | Open contract | Inline partial-column omission, saved-chunk precedence, and exact publisher/player retention interaction remain unresolved. | `crates/client-world/src/stream/residency.rs`, `crates/world/src/chunk_grid.rs` | Use exact edge columns, partial replacements, saved positions, and negative/unaligned centers in a native trace. |
| VPA-106 | Implementation landed | Non-finite remote player poses and unknown equipment containers now produce skippable semantic errors, preserving the prior usable actor/equipment state and session. | `crates/protocol/src/world.rs`, `crates/protocol/src/item.rs`, `crates/protocol/src/login.rs` | Preserve this boundary while wider actor packet, movement, lifetime, and rendering gaps remain open. |
| VPA-107 | Proven mismatch | Correction rotation/delta/mode data is discarded or hard-snapped; camera and replay consume only position/ground. | `crates/protocol/src/world/events.rs`, `app/src/runtime/world.rs`, `app/src/movement/physics.rs` | Define packet-specific correction semantics and test rotation-only, retained-tick, normal, and teleport modes. |
| VPA-108 | Proven mismatch | Consumable slowdown exists in `sim` but production always supplies false; active sprint is treated as the held sprint request. | `crates/sim/src/simulator.rs`, `app/src/movement/physics.rs` | Connect authoritative use lifecycle and processed sprint state to simulation and input flags. |
| VPA-109 | Open contract | Initial prediction tick/ground state, stall tick dropping, packet bursts, third-person camera orientation, and input/DPI sensitivity need exact measurement. | `app/src/runtime/network.rs`, `movement/physics.rs`, `camera.rs`, `crates/input` | Add discriminating startup/stall/perspective/DPI captures before changing clocks or scaling. |
| VPA-110 | Proven mismatch | Item-stack responses drop custom names, filtered names, and durability corrections; inventory cells omit durability and full identity. | `crates/protocol/src/inventory.rs`, `app/src/ui_runtime/inventory_ledger.rs`, `presentation/hud_layout/inventory.rs` | Retain or resync every authoritative response field and use full stack identity for presentation. |
| VPA-111 | Missing surface | Inventory supports only server-authoritative full-stack left-click Take/Place/Swap and generic 27/54 storage. | `app/src/ui_runtime/interaction.rs`, `inventory_ledger.rs`, `crates/protocol/src/inventory/request.rs` | Add typed cells/actions and then survival, creative, crafting, furnace, controller, and touch slices independently. |
| VPA-112 | Proven mismatch | Corner player preview is unconditional and uses a small software pose, not authoritative rig/equipment/action state or a settled screen/visibility contract. | `app/src/ui_runtime/presentation/player_preview.rs`, `publish.rs`, `hud_layout.rs` | Gate the approved surface and source it from the same actor rig/equipment/action snapshot as world rendering. |
| VPA-113 | Proven mismatch | Legacy 64x32, persona/custom geometry, outer layers, and duplicate entity-definition selection are incomplete or silently defaulted. | `crates/render/src/actor.rs`, `crates/client-world/src/actor_animation.rs` | Add explicit layout/candidate identity and fail-visible bounded fallbacks with native player galleries. |
| VPA-114 | Proven mismatch | Every block-entity renderer remains deferred; backing blocks render without chest/sign/banner/bed/skull/beacon-specific presentation. | `assets/block-entity-renderers-v1001.json`, `crates/client-world/src/block_entity_visuals.rs` | Implement one manifest-owned renderer family with NBT/live-update/native gates. |
| VPA-115 | Proven mismatch | Authentication has no sign-out/account switch and malformed cache recovery has no product flow. | `core/authflow`, `core/authcache`, `app/src/menu`, UI menu screens | Add secure quarantine/sign-out plus bounded supervised recovery and native UX evidence. |
| VPA-116 | Proven mismatch | The normal app does not subscribe to the planned control/status surface; lifecycle, pack, transfer, disconnect, and refresh state stay fragmented. | `core/control`, `app/src/menu.rs`, `crates/bridge` | Define a versioned control protocol and replace one-shot helper polling with one owned subscription. |
| VPA-117 | Proven mismatch | Menus are a custom Java-like shell even though the Java exception applies only to gameplay HUD. Settings selection is transient and often inert. | `app/src/ui_runtime/presentation/menu.rs`, `app/src/menu.rs`, `app/src/settings_runtime.rs` | Establish measured Bedrock/resource-pack menu contracts and one persisted settings authority. |
| VPA-118 | Missing surface | Gameplay touch is disabled; menu/controller input bypasses the shared semantic binding authority. | `app/src/ui_runtime/gameplay_touch.rs`, `app/src/menu/input.rs` | Measure and implement touch layout, then route menu/gameplay through semantic controls. |
| VPA-119 | Missing surface | Local world creation, selection, persistence, reload, pause/resume, and Dragonfly lifecycle are absent. | `app/src/menu`, `core/go.mod`, `plan.md` | Add a lifecycle-managed local Dragonfly target and world CRUD before gameplay parity. |
| VPA-120 | Proven mismatch | Normal core shutdown hard-kills the child despite a graceful stdin/control cancellation path. | `app/src/menu.rs`, `core/cmd/bedrock-core/main.go` | Request graceful stop, wait with a deadline, then kill only as fallback; verify final telemetry/cache sync. |
| VPA-121 | Proven mismatch | Windows endpoint ownership is not enforced; packaging is local-development staging without platform-native metadata/signing/install/update coverage. | `core/internal/streamnet/endpoint_windows.go`, `tools/dist` | Add owner-only Windows transport/ACL tests and platform-native installed-launch pipelines. |

## P2 — visual calibration and bounded policy work

| ID | Classification | Current mismatch | Primary evidence | Closure gate |
| --- | --- | --- | --- | --- |
| VPA-201 | Proven mismatch | 2,031 block states use labeled provisional fallback geometry/materials. | `crates/asset-compiler/src/compiler/visuals/fallback.rs`, `tools/visualcoverage` | Zero-fallback, version-current strict gallery with per-family native comparison. |
| VPA-202 | Open contract | Biome tint uses a provisional equal-weight radius-one kernel. | `crates/meshing/src/biome.rs`, `crates/render/src/biome_tint.wgsl` | Abrupt/diagonal biome boundary native measurements plus cross-chunk GPU witness. |
| VPA-203 | Open contract | Lighting transfer, night/ambient floors, AO, and light-channel combination are approximations. | `crates/render/src/lighting.wgsl`, `crates/meshing/src/lighting.rs` | Native day/night/cave/emissive/Nether/End calibration and performance pass. |
| VPA-204 | Open contract | Atmosphere, celestial size/filtering, weather, medium fog, and cloud geometry/configuration are not calibrated. | `crates/render/src/atmosphere*`, `cloud_config.rs`, `crates/meshing/src/cloud.rs` | Matching-view all-phase/horizon/weather/underwater/lava/cloud galleries. |
| VPA-205 | Open contract | Fluid surface weighting, falling heights, UV flow/scroll, lava behavior, and flipbook replication are unverified or dead. | `crates/meshing/src/liquid.rs`, `crates/render/src/liquid.wgsl`, texture upload code | Frame-by-frame water/lava state gallery across seams, flow, falls, and waterlogging. |
| VPA-206 | Proven mismatch | GUI scale settings do not drive live presentation; Auto mixes physical and logical size; platform safe-area acquisition is a zero stub. | `app/src/menu.rs`, `app/src/ui_runtime/presentation`, `publish.rs` | Java HUD GUI 2/3/4/Auto matrix across DPI/Retina plus platform inset propagation. |
| VPA-207 | Proven mismatch | Ordinary actor name tags are absent unless a below-name scoreboard objective exists. | `app/src/ui_runtime/presentation/publish.rs`, actor queries | Bounded distance/occlusion/mode name-tag path with live two-client evidence. |
| VPA-208 | Open contract | Persistent pack cache omits server digest; asset fetch freshness is presence-based; multi-file generators publish non-transactionally. | `core/packcache`, fetch scripts, generator tools | Digest-bound identity and stale/interrupted publication tests. |

## Program rules

1. Fix P0 items as independently reviewable vertical slices. Do not combine unrelated protocol,
   movement, rendering, and product changes into one acceptance claim.
2. Every behavior change starts with an exact contract and a failing regression witness.
3. A deterministic test closes implementation only. Native, visual, timing, performance, and
   review gates remain separate.
4. Do not replace an open contract with a guessed constant. Label a bounded approximation in
   `plan.md` and user-facing status until independent measurement exists.
5. Keep captures, credentials, non-redistributable inputs, and Mojang payloads out of git. Commit
   only independently stated behavior, lawful compact rules, hashes, and tests.
