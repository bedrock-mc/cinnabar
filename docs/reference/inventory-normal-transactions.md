# Server normal inventory transactions

## Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Receive | Postload `InventoryTransaction` descriptors, call the complex transaction with client mode enabled, and refresh UI. |
| Execution | Verify normal transaction actions, then execute their final stacks; balancing/recalculation is server-only. |
| Client verification | Ignore mismatched previous player stacks when client mode is enabled. |
| Player inventory | Source 0/window 0 writes `toItem` to the player inventory. |
| Armor | Source 0/armor writes `toItem` to the selected armor cell. |
| UI | Source 0/UI writes `toItem`; cursor also updates the carried instance; output 50 is specially deferred. |
| Offhand | Source 0/offhand writes `toItem` to its sole cell. |

## Observed failure and correction

Local vanilla BDS sent a normal transaction when a dropped dirt item was picked
up. It did not send `InventorySlot` or `InventoryContent` for that pickup.
The old raw whitelist discarded the transaction, leaving the client at 63 while
the server held 64. A subsequent drop used the stale stack identity and failed
source-slot validation. `TakeItemActor` itself is not inventory authority.

The fresh raw body captured at 00:52 UTC on 2026-10-02 is exactly 90 bytes:
player slot 6 changes dirt 63 to dirt 64 with positive stack ID 81 (`a2 01`),
and a separate world-interaction balancing action consumes one item. The shared
`crates/protocol/fixtures/inventory_transaction_pickup.hex` wraps those untouched
body bytes in a minimal uncompressed batch for both raw-ingress and app-pipeline
regressions. It contains no game assets, credentials, or user messages.

The client now retains each supported action's complete final descriptor:
count, positive server stack ID, metadata, block-runtime bits, opaque extra data
and its digest. It does not replay a Take/Drop or invent a count from an actor
pickup notification. One transaction stays one FIFO inventory event. Its
ordered slot updates write authoritative backing and then refold once, retaining
any newer absolute sparse predictions. Identical player writes still advance
the per-slot authority epoch that invalidates local crossbow predictions.
HUD, equipment/selected-stack views, crafting FIFO/quota/observation and use-on
identity evidence consume the same slot-update surface. Crafting replaces all
affected grid/cursor cells in one committed revision, without synthetic sequence
numbers or a partially published batch on credit refusal.

## Scope and remaining gates

Supported direct writes are player, offhand, armor, and ordinary personal UI
cells, with the vanilla cursor projection. Unknown or unreviewed sources/slots,
odd final descriptors, and writes exceeding the existing inventory retention
limit are counted skips. The known world balancing leg produces no inventory
write and no fabricated unknown-container count. Truncation, envelope failures
and trailing packet bytes remain fatal framing errors.

The vanilla deferred UI output-50 path, arbitrary
open-window transaction execution, and non-normal complex transaction types are
not implemented here. Offhand charged-item attachable context is a separate
existing gap. This patch does not close general inventory/container parity.
Focused regressions and the live acceptance below establish only the supported
receive path.

## Live acceptance

On 2026-10-02 UTC the canonical macOS/Metal client, Retina scale 2, connected
to offline loopback vanilla BDS; version and image come from
`assets/bedrock-target.json`.
No Xbox login or synthetic pickup accounting was used.

- Dirt 64/id 82: Drop -3 Accepted at 00:56:24 gave 63; pickup at 00:56:49
  wrote 64/id 88. The next Drop -5 used id 88 and was Accepted at 00:57:17;
  pickup at 00:57:35 wrote 64/id 89. Both inspected HUD frames
  (`00.56.51.png`, `00.57.36.png`) showed 64 and no remaining dropped actor.
  A read-only BDS count query independently confirmed 64.
- The whole 21-diamond stack was taken and dropped from the inventory cursor
  (-19/-21 Accepted). Its pickup at 00:59:53 wrote 21/id 90 into the server's
  chosen first empty slot, not the former slot. The inspected `00.59.54.png`
  HUD showed 21 there, and subsequent Take/Place -23/-25 used id 90 and were
  Accepted. BDS independently confirmed 21.
- The offhand regression, further unrelated moves, and three inventory reopen
  cycles passed on this same build; see [sparse prediction](inventory-sparse-prediction.md).

Full local frame names are prefixed `2026-10-02_`; they remain outside git.
The inspected drop frames (`00.56.25.png` dirt cube and `00.59.15_1.png`
extruded stacked diamond sprites) also verified ordinary geometry, clipping,
colors, depth/layering, and input behavior on a lit test pad. They do not close
the special-item dropped-model, material or pickup-trajectory parity gaps.
