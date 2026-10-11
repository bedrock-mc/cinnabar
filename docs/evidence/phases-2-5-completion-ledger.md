# Phases 2-5 Completion Ledger

End-rod terrain coverage now includes the six facing states, separate shaft/base sprite
mapping and an original-art GPU fixture. This bounded geometry correction leaves the
exhaustive residual-family, world-join and hardware performance gates open.

## P2.5-NATIVE-BIOME

| Field | Evidence |
|---|---|
| Owning plan/task | Not started |
| Deterministic tests | Not started |
| Review commit | Not started |
| Live/native witness | Not started |
| Performance/resource witness | Not started |
| Final status | Open |

## P2-CHUNK-PUBLICATION

| Field | Evidence |
|---|---|
| Owning plan/task | Not started |
| Deterministic tests | Not started |
| Review commit | Not started |
| Live/native witness | Not started |
| Performance/resource witness | Not started |
| Final status | Open |

## P2.7-ATMOSPHERE

| Field | Evidence |
|---|---|
| Owning plan/task | Numbered raster pack skyboxes and optional native vertical placement in `plan.md`; broader atmosphere work remains open. |
| Deterministic tests | Pack admission and reload dependency regressions in `pack-runtime`, with publication regressions in `app`, cover priority, malformed and oversized winners, missing faces and removal. GPU tests cover projection, filtering, transparency, depth, visibility and stable colours; texture-cache tests cover reloads and dimension changes. |
| Review commit | Skybox tranche tracked in [#632](https://github.com/bedrock-mc/cinnabar/pull/632); full phase review remains open. |
| Live/native witness | Retained-pack offline GPU views and both Video setting states were inspected. Current-server full-client acceptance remains open. |
| Performance/resource witness | Existing raster byte, dimension and allocation bounds remain; texture-cache regressions cover stable frames. Live frame and join budgets remain open. |
| Final status | Open — numbered PNG/TGA/JPEG skyboxes are implemented; HDR/KTX, enhanced rendering and broader atmosphere acceptance remain open. |

## P3-MOVEMENT

Spectator regressions cover forced flight without MayFly, solid and unloaded
terrain traversal, and embedded server anchors. The full native trajectory,
platform, and performance gates remain open.

| Field | Evidence |
|---|---|
| Owning plan/task | `docs/superpowers/plans/2026-07-17-phase-3-movement-controls-camera.md`, Tasks 8-14 |
| Deterministic tests | Current integration tranche: 282/282 `bedrock-client` library tests and strict all-target/all-feature Clippy green; bounded Phase 3 evidence validator 5/5 green. See `docs/evidence/phase-3-movement-controls-camera.md`. Render frame-clock tests also cover 20 Hz tick flush cadence at 60/120 Hz FIFO and uncapped, with manual recording clock preservation; live movement cadence remains unverified. |
| Review commit | Pending independent review of the final integration commit. |
| Live/native witness | Current tranche not run; local BDS, Lunar, Zeqa, LBSG, and matching native gates remain required. |
| Performance/resource witness | Current tranche not run; bounded live JSON records remain required. |
| Final status | Open -- deterministic integration advanced; normal Physics enable and binding live evidence remain gated. |

Runtime queue ownership, publication budgeting, world-service schedules, shutdown,
session helpers and pack geometry preparation now belong to `client-runtime`.
App adapters retain menu, movement, presentation and audio integration. Existing
regressions follow the moved owners or continue exercising app composition; this
extraction is incomplete and adds no compiled, executed or live parity evidence.

## P3.4-INPUT-CAMERA

Frame sets and shared player, settings and finalized-input resources now live in
`client-state`. The shell publishes focus for gameplay and camera input; the input
and cursor regressions retain their domain owners. This ownership change has not
been compiled or executed, and does not close this gate.

`client-extensions` now owns personal component collection and callback merging,
permission grants, player snapshots, panel fonts, and server consent input and text.
Consent state and ordering live in `client-state`. The coupled session, gameplay,
menu, render and audio adapters remain in `app`; the extraction and its execution
validation are incomplete. The moved tests retain their assertions, and this move
closes no acceptance gate.

Spectator hand suppression and no-clip inside-block overlays have owning
behavioral regressions. Runtime skin controller visibility precedes equipment, and
authored opacity has a GPU regression. The spectator material fallback remains
provisional; version-matched stock materials, custom controller geometry, persona,
and exact native and platform comparison remain open.

| Field | Evidence |
|---|---|
| Owning plan/task | `docs/superpowers/plans/2026-07-17-phase-3-movement-controls-camera.md`, Tasks 8-10 and 12-14 |
| Deterministic tests | Atomic interaction identity/reset, real eleven-stage production schedule membership/order, semantic device barriers, all perspectives/camera collision, and gameplay movement/jump/use/look touch targets are green in the 282-test client suite. |
| Review commit | Pending independent review of the final integration commit. |
| Live/native witness | Current tranche not run; touch, first/rear/front, collision, local-avatar, and native comparison gates remain required. |
| Performance/resource witness | Current tranche not run; 30/60/144 FPS and bounded resource artifacts remain required. |
| Final status | Open -- deterministic code and validator present; binding live/native evidence remains gated. |

## P4.3-RIGS

| Field | Evidence |
|---|---|
| Owning plan/task | `plan.md`: local custom models; static imports reuse actor preparation. |
| Deterministic tests | Focused custom-import, rollback, persistence, login/update encoding, preview-worker and shared first/third-person preparation tests pass. |
| Review commit | The PR description records independent review of static imports; full parity review remains open. |
| Live/native witness | Headless before/after preview and local-avatar frames, restart persistence and second-client login/update frames are attached to the PR; complete native parity remains open. |
| Performance/resource witness | Worker preparation is bounded; hardware frame budgets remain open. |
| Final status | Open |

## P4.4-LIVE-ACTOR

Stock wolf texture admission preserves controller-selected adult/baby variants
and dye-mask alpha bytes. Tamed collar colors follow byte dye metadata and live
updates. Wet shading, custom-material composition, and exact target-version stock
material/shader linkage remain open.

| Field | Evidence |
|---|---|
| Owning plan/task | Not started |
| Deterministic tests | Not started |
| Review commit | Not started |
| Live/native witness | Not started |
| Performance/resource witness | Not started |
| Final status | Open |

## P4.5-ITEM-ACTIONS

| Field | Evidence |
|---|---|
| Owning plan/task | `plan.md`: Totem activation; broader item actions remain open. |
| Deterministic tests | Local/remote event admission, equipped-item snapshot across delayed publication, equipment-only hands, consumption ordering, retrigger, expiry, session retirement, partial ticks, bounded stall time, activation before the frame clock update, full-viewport scissor and retained clipping, viewport/layer placement, admitted texture replacement/removal and authored particle routing/attachment. |
| Review commit | Totem implementation: `590c2039e`; broader item actions remain open. |
| Live/native witness | Totem camera event/icon behavior and local authored-emitter routing verified; full-client default/custom activation, main-hand fallback, retrigger and remote/empty exclusion pass. Broader item actions remain open. |
| Performance/resource witness | Not started |
| Final status | Open |

## P5.1-UI

| Field | Evidence |
|---|---|
| Owning plan/task | `plan.md`: Compact font carriers and glyph residency; broader UI parity remains open. |
| Deterministic tests | Font schema, bounded decode, frame residency, atlas churn, and retained allocation regressions cover compact carriers. Original large-sheet UV, loop, reload and independent residency-budget regressions cover tiled UI images. |
| Review commit | Not started |
| Live/native witness | Exact coverage/SDF GPU pixels and mixed-script offline frames; native Bedrock UI parity remains open. |
| Performance/resource witness | Font storage is bounded and development-profile first-menu startup is measured; release hitch and hardware-tier qualification remain open. |
| Final status | Open |

Sound engine, session, named playback, local cue and UI sound tests belong to
`client-audio`; camera publication and actor presentation belong to
`client-presentation`. App integration tests retain the composed frame-order and
camera-listener coverage. This ownership change adds no new parity evidence.

## P5.2-HUD

| Field | Evidence |
|---|---|
| Owning plan/task | Not started |
| Deterministic tests | Not started |
| Review commit | Not started |
| Live/native witness | Not started |
| Performance/resource witness | Not started |
| Final status | Open |

## P5.3-CHAT

| Field | Evidence |
|---|---|
| Owning plan/task | Not started |
| Deterministic tests | Not started |
| Review commit | Not started |
| Live/native witness | Not started |
| Performance/resource witness | Not started |
| Final status | Open |

## P5.4-SCOREBOARD

| Field | Evidence |
|---|---|
| Owning plan/task | Not started |
| Deterministic tests | Not started |
| Review commit | Not started |
| Live/native witness | Not started |
| Performance/resource witness | Not started |
| Final status | Open |

## P5.5-INTERACTION-COMBAT-INVENTORY

| Field | Evidence |
|---|---|
| Owning plan/task | Partial local placement coverage; see the Local placement prediction section in plan.md. |
| Deterministic tests | Gameplay direction, support, stacking and immediate-placement tables in both palette encodings; pipeline atomic pair, rollback and ordered publication tests. Item-use tests cover frame/tick slot changes, the native rearm boundary, category cooldowns, rejected-send rollback and hotbar cooldown publication. |
| Review commit | Recorded by the placement follow-up PR. |
| Live/native witness | Headless stair comparison and door pair before server confirmation; broader family visual parity remains open. |
| Performance/resource witness | Paused-server stair recording: first visible/staged +3 frames, observed upload +5; bounded late worker service. Click-frame and full hardware budgets remain open. |
| Final status | Open: remaining placement families and the wider interaction/inventory gate are incomplete. |

## P5.6-FORMS

| Field | Evidence |
|---|---|
| Owning plan/task | Not started |
| Deterministic tests | Not started |
| Review commit | Not started |
| Live/native witness | Not started |
| Performance/resource witness | Not started |
| Final status | Open |

## P5.7-PARITY-PERF

| Field | Evidence |
|---|---|
| Owning plan/task | Bamboo geometry, column transforms, sampling and overlay consumers covered; wider parity and performance work remains open |
| Deterministic tests | All twelve bamboo states, offset admission in both ID spaces, source-pixel mip selection, opaque/cutout consumers and populated Metal rendering graph |
| Review commit | Not started |
| Live/native witness | Fresh Vanilla and bounded Enhanced bamboo gallery, selection and timed breaking captures; screenshots accompany the change |
| Performance/resource witness | No displayed-frame performance acceptance from hidden debug captures; ordinary Enhanced remains disabled |
| Final status | Open |

## P5.8-SETTINGS

| Field | Evidence |
|---|---|
| Owning plan/task | `launcher` settings models; `launcher-host` storage and persistence; `app` runtime adapters. Persisted **Bedrock Skybox Vertical Offset** Video option; broader settings completion remains open. |
| Deterministic tests | Launcher regression covers default-on, old saves, both saved values, subsystem projection and Video reset. Offline JSON-UI regression reaches and renders both toggle states. |
| Review commit | Skybox option tracked in [#632](https://github.com/bedrock-mc/cinnabar/pull/632); full phase review remains open. |
| Live/native witness | Both offline Video control states were inspected; full settings and current-server acceptance remain open. |
| Performance/resource witness | Full settings performance witness remains open. |
| Final status | Open — the skybox option is implemented; the complete settings gate is not closed. |

The standalone setup window belongs to `first-run`. Its preparation tests move with it;
menu integration tests stay in `app`. Account, process and persistence tests belong to
`launcher-host`. These ownership changes do not close the settings parity gate.

## Xbox presence follow-up

The account core publishes menu, world-default, Realm and generic featured/Experience activity.
Exact heartbeat, platform configuration, server overrides, permission gates and
friends-visible acceptance remain incomplete. No existing completion entry is
advanced by this feature; see `docs/reference/xbox-presence.md` and `plan.md`.

## Session block highlight follow-up

Highlight lookup uses admitted custom state identities and the pinned base registry.
Regression coverage includes both ID modes, palette replacement and session reset.
The loaded-block highlight capture shows a custom block before and after admission.
Selection shape parity and displayed-frame performance gates remain open.

## Inherited block overlay geometry

Block overlays use the shared pack compiler to resolve legacy geometry inheritance before projecting faces. Parents may come from other model files or pack layers; missing parents, cycles and chains beyond the depth limit are rejected. Budget-limited reads admit higher-priority definitions first. No existing completion entry is advanced by this change; the remaining gates are recorded in `plan.md`.

## Session biome climate and environment

Admitted live biome identifiers and climate now travel with the session tint registry. Weather, fog precipitation, camera temperature and environment routes read that registry, including custom biome routes. Registry replacement clears removed routes. No existing completion entry is advanced by this change; the remaining gates are recorded in `plan.md`.
