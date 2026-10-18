# Cinnabar roadmap

This file tracks current work and acceptance gates. Change history comes from
[PR titles and release notes](CHANGELOG.md); implementation details live with their
[crate owners](docs/architecture/workspace.md) and the topic notes below.

## How to update this roadmap

- Update the owning phase or topic section when scope, remaining work or acceptance changes.
- Keep provisional behavior and missing evidence explicitly incomplete. A merged PR,
  passing unit test or ownership refactor does not by itself close a parity gate.
- Record fresh verification with the revision and scope it proves. Keep internal research
  and agent reports in the private maintainer checkout described in [AGENTS.md](AGENTS.md).
- Keep the phase requirement IDs below consistent with the
  [completion ledger](docs/evidence/phases-2-5-completion-ledger.md). The architecture
  check requires each ID exactly once in its owning phase here.
- Use a brief imperative PR title and one category label for the change record.
  Do not append a per-PR entry to this file or maintain a second handwritten changelog.

## Current priorities

The project runs a client, local worlds, online services and several experimental
hosts. Implementation coverage and accepted vanilla parity remain separate.
The open work is to finish version-matched behavior and presentation, qualify
frame/streaming/join budgets, and prove the supported platform and session paths.
The phase checklists retain their original checked and unchecked items; this
reorganization changes no completion status.

| Area | Remaining acceptance and implementation work | Details |
| --- | --- | --- |
| Terrain and rendering | Finish remaining materials and geometry, biome/environment behavior, bounded publication and native visual/performance acceptance. | [Phase 2](docs/roadmap/2-world-rendering.md) |
| Movement and interaction | Finish input/camera, prediction, correction, item actions and server acceptance. | [Phase 3](docs/roadmap/3-movement.md), [Phase 5](docs/roadmap/5-interaction-ui.md) |
| Actors and equipment | Finish animation/pose, held items, live actor and artwork acceptance. | [Phase 4](docs/roadmap/4-actors.md) |
| UI and menus | Finish pack-driven screens, HUD/chat/forms, inventory and settings acceptance. | [Phase 5](docs/roadmap/5-interaction-ui.md) |
| Platforms and product | Qualify browser/Android experiments, online services, local worlds, audio and packaging. | [Platform status](docs/roadmap/status-platform-services.md), phases 6–8 below |
| Ownership cleanup | Simplify existing owners and integration without treating code movement as behavior evidence. | [Architecture status](docs/roadmap/status-architecture.md) |

## Topic status

These notes retain implementation progress and explicit limitations previously
mixed into this file. A topic may be implemented while its broader acceptance
remains open. Use the phase checklist and completion ledger together with it.

- [Rendering and artwork](docs/roadmap/status-rendering.md)
- [World and gameplay](docs/roadmap/status-world-gameplay.md)
- [UI and screens](docs/roadmap/status-ui.md)
- [Platforms and services](docs/roadmap/status-platform-services.md)
- [Architecture cleanup](docs/roadmap/status-architecture.md)
- [Mods, experiences and cosmetics](docs/roadmap/status-extensions.md)

## Phase 0 — End-to-end acceptance

- [ ] Complete the streamed-world interaction and performance acceptance run.

[Retained spike checklist](docs/roadmap/0-spike.md).

## Phase 1 — Core service

The retained checklist records the implemented Go service boundary. Further
service changes still require focused tests and current session acceptance.

[Core checklist](docs/roadmap/1-core.md).

## Phase 2 — World rendering

- [ ] `P2.5-NATIVE-BIOME` — Complete biome palettes, tinting and native acceptance.
- [ ] `P2-CHUNK-PUBLICATION` — Qualify initial publication and steady streaming budgets.
- [ ] `P2.7-ATMOSPHERE` — Complete lighting, weather, sky and atmosphere acceptance.

Other material, geometry and gallery items remain open in the
[world-rendering checklist](docs/roadmap/2-world-rendering.md).

## Phase 3 — Movement and the local player

- [ ] `P3-MOVEMENT` — Complete movement, prediction, correction and live server acceptance.
- [ ] `P3.4-INPUT-CAMERA` — Complete semantic input and camera acceptance.

[Movement checklist](docs/roadmap/3-movement.md).

## Phase 4 — Entities and equipment

- [ ] `P4.3-RIGS` — Complete data-driven rigs and animation acceptance.
- [ ] `P4.4-LIVE-ACTOR` — Complete the live actor contact and interpolation witness.
- [ ] `P4.5-ITEM-ACTIONS` — Complete held items, actions, dropped items and viewmodel acceptance.

[Actor and equipment checklist](docs/roadmap/4-actors.md).

## Phase 5 — Interaction, inventory and UI

- [ ] `P5.1-UI` — Complete the pack UI foundation and acceptance.
- [ ] `P5.2-HUD` — Complete server text and HUD behavior.
- [ ] `P5.3-CHAT` — Complete interactive chat acceptance.
- [ ] `P5.4-SCOREBOARD` — Complete scoreboard and boss-bar acceptance.
- [ ] `P5.5-INTERACTION-COMBAT-INVENTORY` — Complete interaction, combat and inventory acceptance.
- [ ] `P5.6-FORMS` — Complete server form behavior and acceptance.
- [ ] `P5.7-PARITY-PERF` — Complete matched UI parity and performance qualification.
- [ ] `P5.8-SETTINGS` — Complete menu, control, video and persistence acceptance.

[Interaction and UI checklist](docs/roadmap/5-interaction-ui.md).

## Phase 6 — Online product surface

[Online checklist](docs/roadmap/6-online.md). Retained completed steps do not close
all reconnect, identity, service or platform acceptance.

## Phase 7 — Local worlds

[Local-world checklist](docs/roadmap/7-local-worlds.md). Keep the accepted
local-world implementation scope separate from full vanilla world-generation parity.

## Phase 8 — Audio, polish and packaging

[Shipping checklist](docs/roadmap/8-shipping.md). Validate current audio, startup,
packaging and platform behavior before claiming a supported release.

## Working rules and evidence

Current [repository instructions](AGENTS.md), the [workspace ownership map](docs/architecture/workspace.md)
and [live-testing guide](docs/agents/live-testing.md) govern implementation and verification.
The [retained acceptance requirements](docs/roadmap/acceptance.md) keep the sequencing,
release definition and rendering budgets from the phase plan.
Use [release history](CHANGELOG.md) for what landed and the
[completion ledger](docs/evidence/phases-2-5-completion-ledger.md) for acceptance evidence.
Earlier roadmap revisions remain in Git history; dated execution diaries and
superseded integration snapshots are not current implementation instructions.
