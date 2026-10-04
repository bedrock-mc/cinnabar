# Crafting and cursor return on screen close

## Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Return on close | Visit each return-on-close controller and each of its slots, returning the item to the player or dropping it. |
| Overflow | Auto-place into the player inventory, then Drop what cannot fit. Ingredients are real inventory, not disposable UI previews or assumed server-side returns. |
| Controller scope | Cursor and crafting input controllers return items on close; the combined player inventory does not. Both carried items and crafting inputs need cleanup. |

## Correction

Personal and workbench closes stage cleanup against the same sparse inventory
view used by ordinary transfers. Each input/cursor stack fills compatible partial
player stacks, then empty cells; any overflow becomes a Drop action. The plan is
atomic locally: invalid identities, unavailable authority or queue pressure do
not publish only half a cleanup. Requests retain their prior sparse dependencies.

Close transport waits for those requests and their predecessors to settle and
for the confirmed grid/cursor to be empty. Empty sparse cells with unanswered
requests still require settlement; cancelling them can resurrect an ingredient
in backing inventory. A rejected or incomplete return cancels the pending Close
and restores the retained open surface so the item can be recovered. A close
notification never silently erases an unexpectedly occupied crafting cell.

This acknowledgement-before-Close ordering is Cinnabar's bounded admission
policy, not a claim of identical native tick/flush timing. Arbitrary container
return flags, structural NBT merging, native collection ordering and every
server-initiated-close recovery path remain outside this scoped change.

Regressions cover personal/workbench return, partial-stack plus empty-slot
distribution, full-inventory overflow, cursor overlays, dependent unsent/admitted
requests, invalid identities, rejection recovery and reopen. BDS acceptance is
recorded in [the correction acceptance record](../reviews/inventory-hud-crafting-fixes.md),
including Accepted input/cursor returns, inspected reopened screens and
independent server counts.
