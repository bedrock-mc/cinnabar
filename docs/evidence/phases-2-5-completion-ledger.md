# Phases 2-5 Completion Ledger

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
| Owning plan/task | Not started |
| Deterministic tests | Not started |
| Review commit | Not started |
| Live/native witness | Not started |
| Performance/resource witness | Not started |
| Final status | Open |

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

## P3.4-INPUT-CAMERA

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
| Owning plan/task | Not started |
| Deterministic tests | Not started |
| Review commit | Not started |
| Live/native witness | Not started |
| Performance/resource witness | Not started |
| Final status | Open |

## P5.1-UI

| Field | Evidence |
|---|---|
| Owning plan/task | `plan.md`: Compact font carriers and glyph residency; broader UI parity remains open. |
| Deterministic tests | Font schema, bounded decode, frame residency, atlas churn, and retained allocation regressions cover compact carriers. |
| Review commit | Not started |
| Live/native witness | Exact coverage/SDF GPU pixels and mixed-script offline frames; native Bedrock UI parity remains open. |
| Performance/resource witness | Font storage is bounded and development-profile first-menu startup is measured; release hitch and hardware-tier qualification remain open. |
| Final status | Open |

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
| Owning plan/task | Not started |
| Deterministic tests | Not started |
| Review commit | Not started |
| Live/native witness | Not started |
| Performance/resource witness | Not started |
| Final status | Open |

## Xbox presence follow-up

The account core publishes menu, world-default, Realm and generic featured/Experience activity.
Exact heartbeat, platform configuration, server overrides, permission gates and
friends-visible acceptance remain incomplete. No existing completion entry is
advanced by this feature; see `docs/reference/xbox-presence.md` and `plan.md`.
