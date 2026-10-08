# Held block placement

These are the Bedrock held-use rules for the matched client version. The hold tracks
successful destinations, an optional placement line, and the initial world click.
The ordinary cube path follows this table; specialized block placement and prediction
remain subject to each item's placement rules.

| Vanilla rules | Behaviour |
| --- | --- |
| First press | Attempt the fresh clicked block and face immediately. Start a new hold with no previous success or line. |
| Repeats | Refresh the world pick each simulation tick. An attempt is due only when elapsed time is strictly greater than the repeat delay; this is not a fixed tick count. |
| Slow delay | 300 ms while sneaking, interacting with a block, or placing before a line has been established. At 20 Hz and an exactly aligned timestamp, the first due opportunity is tick 7. |
| Ordinary delay | 200 ms while still. While moving, truncate `min(900 / speed, 180)` milliseconds, where speed is the length of the actual post-tick movement delta multiplied by 20. Every noncreative mode has a 100 ms minimum. |
| Timing after attempts | Failed attempts retain the previous success time and may retry next tick. Successful moving repeats advance the scheduled time, limited to 180 ms of catch-up; stationary success records the current time. |
| Direction before a line | After a successful placement, actor velocity with squared length greater than 0.01 chooses the largest absolute movement axis; ties choose Z. Without sneaking, use the last successful destination as the support and this axis as the face. |
| Qualifying blocks | The held block must allow the placement intention. Ordinary cubes, stairs, slabs, fences, thin fences/panes, walls and carpet qualify, as do soul sand, mud, barriers and chiseled bookshelves through inherited cube properties; a nonzero block runtime ID alone is insufficient. Custom block placers also control whether this intention is enabled. |
| Acquiring a line | An adjacent attempted destination after a previous success fixes the direction and face while use remains held. An attempt can establish the line even if it fails. A successful attempt at the next destination advances that destination by the fixed direction. |
| Target while lined up | Intersect the fresh pick segment with the next unit block cell. If it intersects, use the preceding cell as support and the locked face. A miss uses the full reach segment, allowing continuation beyond a ledge without seeing the support's side. |
| Unlocked target | A valid solid block pick is required before the hold has a line. There is no unconditional placement at the player's feet. Item replacement rules determine whether the destination is the clicked block or its adjacent cell. |
| Sneaking | Retain the fresh picked support, apply the movement-selected face, use the slow delay, and do not establish or advance a placement line. Normal sneaking movement prevents walking beyond a supporting edge. |
| Jumping and towering | There is no grounded-only placement check. Jumping can clear the player's collision box for a block underneath; upward movement can select the upward face and establish a vertical line. Ray, collision, replacement and item rules still apply. |
| Reach | Mouse: 5.7 blocks; gamepad: 5.6. Touch: 6.7 in survival and 12 in creative. The pick segment continues to limit a locked line. |
| Click position | A fresh hit keeps its world intercept relative to the selected support, even when the support changes. A miss continuation uses zero. Orientation-sensitive blocks keep the first successful world intercept throughout the hold; relative offsets may therefore lie outside the unit cube. |
| Target type changes | A block, actor or ray miss does not reset the hold. An unusable or occluded pick suspends attempts while preserving the successful destination and line. A block interaction disables placement intention until a qualifying placement resumes it. |
| Movement corrections | A same-session correction pauses attempts until fresh movement evidence is available, preserving the successful destination, line and repeat schedule. |
| Selected stack during continuation | Each repeat reads the current selected stack and applies its placement rules. The continuation itself does not reset retained history by comparing stack identity. A refused repeat does not advance the success clock. |
| Hold reset | Release, attack and blocked gameplay input such as a menu stop the hold. Stop reports the last successful destination; a new press starts fresh. Blocked gameplay also cancels deferred presses. |

Cinnabar provisionally preserves the successful destination, locked direction/face,
first intercept and repeat schedule across slot or item changes. Unconfirmed selection
pauses attempts and retains deferred presses. Whether native selection callbacks reset
that history or timing remains incomplete; see [the open gate](../../plan.md#held-block-placement).

The delay is measured in milliseconds rather than counted ticks. For example, a
100 ms threshold becomes eligible on the third exactly aligned 50 ms opportunity,
and a 200 ms threshold on the fifth. Actual opportunities depend on simulation timing.

| Outbound rules | Behaviour |
| --- | --- |
| Placement transaction | Every attempted use sends a standalone item-use inventory transaction with action `Place`. Building is not a `PlayerAuthInput` destruction block action. |
| Trigger | The first press uses `PlayerInput`; held repeats use `SimulationTick`. |
| Target evidence | Send the selected support position, face, support runtime ID before prediction, current network player position, and relative click position. |
| Prediction | Report success only when local use succeeds; otherwise report failure. The client cooldown flag is off. |
| First successful use | Send `StartItemUseOn` with the support, calculated destination and face before the swing and transaction. Held repeats do not send another start action. |
| Stopping | Send `StopItemUseOn` with the last successful destination, zero result position and face zero. |
| Survival inventory | Include the selected inventory-slot delta. A nonempty decremented stack receives the predicted negative legacy request ID and matching legacy slot record. An emptied stack has request ID zero and no legacy slot record. |
| Server corrections | Nonempty legacy placement uses negative even IDs and authoritative slot/content restatements. `ItemStackResponse` acceptance/rejection applies to registered negative odd item-stack requests; unknown IDs do not alter inventory. |
| Initial air fallback | After a plain block-use press, attempt air use with the remaining selected stack. There is no air fallback when the stack emptied, and block-item holds do not repeat this air use. |
| Creative inventory | Do not decrement the held count or include a count-change delta. |

See [block placement prediction](block-placement-prediction.md) for destination and
local world prediction admission. The table does not close specialized-block or live
server acceptance gates.
