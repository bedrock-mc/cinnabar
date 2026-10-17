# Crafting and cursor return on screen close

## Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Return on close | Visit each return-on-close controller and each of its slots, returning the item to the player or dropping it. |
| Overflow | Auto-place into the player inventory, then Drop what cannot fit. Ingredients are real inventory, not disposable UI previews or assumed server-side returns. |
| Controller scope | Cursor and crafting input controllers return items on close; the combined player inventory does not. Both carried items and crafting inputs need cleanup. |

## Correction

Personal inventory and storage-window closes stage cleanup against the same
sparse inventory view used by ordinary transfers. Each input/cursor stack fills
compatible partial player stacks, then empty cells; any overflow becomes a Drop action. The plan is
atomic locally: invalid identities, unavailable authority or queue pressure do
not publish only half a cleanup. Requests retain their prior sparse dependencies.

Close transport follows the cleanup requests and their predecessors without
waiting for their answers. Closed window generations retain unanswered requests
so replies can still settle their cursor and player cells. Empty sparse cells
with unanswered requests still need settlement; cancelling them can resurrect
an ingredient in backing inventory. A close notification never silently erases
an unexpectedly occupied crafting cell. Inputs that cannot be returned retain
the existing authoritative recovery requirements.

Chest and server-menu closes use the same cursor cleanup, including Drop when
the player inventory is full. Previously that path only ran for workbenches and
client-authoritative storage, leaving a held chest cursor to require recovery
and block later inventory gestures and world interactions.
Storage cursor cleanup does not move retained inputs from an inactive personal
crafting grid, and an invalid identity there cannot abort the cursor return.

Arbitrary container return flags, structural NBT merging, native collection
ordering and every server-initiated-close recovery path remain outside this scoped change.

Regressions cover personal/workbench return, partial-stack plus empty-slot
distribution, full-inventory overflow, cursor overlays, dependent unsent/admitted
requests, invalid identities, rejection recovery and reopen. Storage-close tests
also cover accepted and unanswered takes, both close-acknowledgement orders,
and successful inventory gestures after reopening. Earlier BDS acceptance is
recorded in [the correction acceptance record](../reviews/inventory-hud-crafting-fixes.md),
including Accepted input/cursor returns, inspected reopened screens and
independent server counts.
