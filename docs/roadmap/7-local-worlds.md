# Phase 7 — Local worlds on dragonfly

[Roadmap](../../plan.md) · [Acceptance ledger](../evidence/phases-2-5-completion-ledger.md)

This is the retained phase checklist. Checked items describe their recorded scope only;
unchecked items and explicit limitations remain open. Historical verification statements
are not fresh test results. Follow current repository guidance when running checks.


Scope: core embeds/spawns dragonfly (`platform/pc-server` and dragonfly-server skill patterns
as reference); world create/select/delete UI; settings (name, gamemode, seed, flat/normal);
LevelDB world persistence via dragonfly; pause/resume semantics on window focus; same client
path as online (core points the session endpoint at the local dragonfly). Documented v1 limits:
dragonfly's generation and mob AI parity gaps are accepted, not chased.

Status: provisional (see `docs/local-worlds.md`): BDS 1.26.52.3 (native, or the manifest-pinned container on macOS) for default worlds, dragonfly for Flat worlds; menu create/edit/delete/templates and staged loading are built. Live-verified on macOS through the core's control channel (create, staged open, spawn, server-side teleport and client fall, pause, close with no orphan); the menu click-through and on-screen input were not exercised (locked screen), and the create screen lacks vanilla's Multiplayer, Cheats and pack tabs, so no acceptance gate is closed.

Local-world edits save before Play and on Back or Escape. Save errors preserve the form for retry.
Dragonfly restores the local host with the selected mode, and native and container BDS launches
apply the selected mode to existing worlds. The local-world parity gate remains provisional.
