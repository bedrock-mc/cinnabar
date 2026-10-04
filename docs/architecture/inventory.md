# Inventory and local-player authority: restructuring step 2

`inventory` owns the inventory ledger, predictions, request reconciliation, window
generations, equipment routing and hotbar selection. It also owns the credited
crafting projection, recipe matching and crafting request planning. These moved
from `app/src/ui_runtime` and the matching portions of `protocol`. Protocol keeps
wire decoding, packet encoding and immutable decoded recipe definitions.

The crate has no engine, UI, renderer or world-state dependency. Its only local
production dependency is `protocol`. The app's `PlayerRuntime` resource composes
`inventory::InventorySession` and `client_world::LocalPlayerFacts`. `UiRuntime`
retains screen state and presentation, and receives explicit player borrows for
queries and commands. Existing UI methods are temporary adapters; none owns a
second ledger, recipe authority or local-player fact store.

`LocalPlayerFacts` owns session-bound abilities, resolved game modes, retained
block-breaking negotiation, normalized hunger and the current mount. Movement
reads it directly, together with predicted armor from the inventory ledger and
the world stream. HUD hunger and mount values remain presentation projections.
The attribute normalizer is defined once below UI; its rounding, scale and
malformed-value retention are unchanged, including movement's existing comparison
against the quantized hunger current value.

## Synchronous seams and ordering

- Ingress validates the inventory session, FIFO sequence and queue bound, then
  notifies crafting before enqueueing. Authority loss therefore invalidates its
  preview at the same point as before.
- `InventorySession::apply_next` observes one event in crafting before applying it
  to the ledger. The app projects that event into HUD, screen state and observation
  evidence before applying the next event. `finish_drain` advances crafting once,
  after the queue is empty.
- UI gestures issue synchronous ledger commands. Their predictions are visible to
  physics in the same update. No new channel or frame-delayed command queue exists.
- Committed UI identity checks still precede hunger and mount mutation. Abilities
  keep their independent bootstrap binding and sequence fence. Session replacement
  resets both domain owners together; terminal retirement preserves its previous
  narrower clearing behavior.
- Inventory send polls timeouts and projects any screen closure first, then asks
  inventory to send ready packets. Controls precede mutations, queue refusal retains
  the same batch, and transport admission is recorded only after successful send.
- Hotbar input queues selection on `PlayerRuntime.inventory`. Its
  `pending_hotbar_packet` query waits for known stack authority and resolved
  prediction IDs, while the selected slot remains visible immediately. A server
  stack ID of `-1` remains sendable. The app clears the queued slot only after
  transport accepts its packet; retries rebuild it from current inventory state.
- Presentation still captures the pre-send ledger and screen state. The existing
  scoped swap restores the post-send owner on success, error or unwind. Rendering
  receives only shared borrows of that temporary projection.

## Enforcement and evidence

`tools/architecture/policy.toml` registers inventory with `protocol` as its only
allowed local dependency. Its transitive rules forbid app, `ui`, `json-ui`, render,
`world`, `client-world`, Bevy and wgpu packages, including `bevy_*` and `wgpu*`
component packages. Client-world cannot acquire UI, render or engine dependencies.
The dependency gate follows first-party and declared vendored path manifests. It
checks every target table, the origin's normal/build/dev dependencies and downstream
normal/build dependencies. Renaming a dependency does not hide its package name;
undeclared local paths fail closed. Registry dependencies are checked by package
name, without expanding the registry's dependency graph. `verify-affected` uses
Cargo's exact workspace package IDs, so the registry package also named `inventory`
cannot make scoped checks or doctests ambiguous. `verify-affected` passes
Cargo metadata's workspace package IDs to Cargo, so the registry crate also named
`inventory` cannot make scoped verification ambiguous.

The module gate parses Rust syntax. Production movement cannot import `ui_runtime`
or `ui`. Production UI struct, enum and union fields cannot own `InventorySession`,
`PlayerInventoryLedger`, `LocalPlayerFacts`, `AbilitiesUpdate` or `PlayerRuntime`.
It follows local and cross-file type/import aliases and nested generic containers;
references remain borrowed views. The sole ledger exception is the existing
`PresentationInventory` display snapshot. Comments, strings and test-only modules
are excluded. Out-of-line modules inherit their parent declaration's `cfg(test)`,
while a production declaration stays checked even if its filename ends in
`_tests.rs`. This is a syntax gate, without macro expansion or Rust's full type
resolution. Regression fixtures cover these boundaries, dependency kinds, vendor
cycles, alias ownership and test exclusions.

This migration adds no new vanilla behavior or parity claim. The ownership and
projection roles retain the following behavior.

## Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Abilities | The screen model retrieves the local actor's abilities with layer precedence. |
| Equipment | Equipment is updated before the UI notification. |
| Inventory commands | Screen commands and request reconciliation have distinct roles. |

- Installed vanilla pack
  `.local/assets/bedrock-samples/v1.26.50.4/full/resource_pack/ui/hud_screen.json:570`:
  the hunger renderer is gated by the survival-UI projection.

Moved tests retain exact request-byte fixtures, prediction/reconciliation and
window-lifetime coverage. App tests cover the actual committed FIFO drain,
same-frame physics reads, transport pressure and pre-send snapshot restoration.
Build measurements are recorded in the [step 2 timing evidence](../evidence/inventory-build-timings.md).
