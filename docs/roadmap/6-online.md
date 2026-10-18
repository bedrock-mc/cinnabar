# Phase 6 — Online product surface

[Roadmap](../../plan.md) · [Acceptance ledger](../evidence/phases-2-5-completion-ledger.md)

This is the retained phase checklist. Checked items describe their recorded scope only;
unchecked items and explicit limitations remain open. Historical verification statements
are not fresh test results. Follow current repository guidance when running checks.


The marketplace, its store services and purchases, Marketplace Pass inbox category,
and the global Subscriptions settings tab are removed by product policy. Server
resource packs, skins/Dressing Room and Realms remain available. Store packets are
still wire-checked, counted and discarded without opening UI. Navigation, remaining
category icons and local template creation retain their ordering. Compilation and
rendered-frame acceptance of this visible removal remain unverified; no parity
or performance gate closes with it.

Scope: main menu + settings (video/controls/audio/account); server browser (saved servers);
Realms and friends lists in-UI (control-channel data from Phase 1) with one-click join;
transfer/reconnect UX; **server resource packs applied at runtime** (cache from core →
`crates/assets` hot-swaps textures/models/sounds/lang over the vanilla base — the asset
system from Phase 2 must have been built pack-stack-aware); disconnect screens with real
reasons; auth/device-code UX polish. Optional stretch: Lunar module toggles surfaced in-client
via control channel (v1.x, not v1).
