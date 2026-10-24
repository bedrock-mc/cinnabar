# Server Experience runtime

For a paired Rust mod, use [the author guide](mods/README.md). This reference is
for server callbacks, saved data and hosts of `experience-runtime`.

## Running and artifacts

`make local-server` builds `bedrock-local-server`, `experience-runtime` and the
bundle verifier. Installed `.cmod` packages are discovered under `<world>/mods/`.
For unpackaged server artifacts:

```sh
bedrock-local-server -dir <world> -addr 127.0.0.1:19132 \
  -experiences <artifacts> -experience-runtime <binary>
```

Each immediate artifact directory contains `experience.toml`, `server.wasm` and
`assets/`. The manifest declares `id`, `version`, `api`, `data-schema` and `[files]`,
which indexes every other file with a lowercase SHA-256. Paths are relative,
slash-separated and cannot contain traversal or symlinks. `[client]` is ignored by
the server runtime; the bundle builder validates it separately. Other unknown keys
are refused. See [manifest validation](../crates/experience-runtime/src/manifest.rs).

The module must implement its declared server WIT. Client components, WASI,
unknown imports and serialized native Wasmtime artifacts fail loading. Hashes
prove integrity, not trust; install server artifacts as carefully as server code.
Helpers run as the server's OS user without an OS sandbox.

All artifacts load and register before listening. Duplicate IDs, a missing installed
Experience or a missing saved block definition fail startup. `minecraft` is reserved.
Do not edit an artifact during startup: texture registration follows manifest checking.

## Callbacks

Implement `experience_sdk::server::Experience`. A paired project's `#[server]`
entry point owns registration and registry replies; low-level guests can use
`export_experience!`. [SDK contracts](../crates/experience-sdk/src/server.rs) and
[server WIT](../crates/experience-sdk/wit/server/server.wit) define the types.

| Callback | When and scope |
| --- | --- |
| `registration` / `register` | Startup; declares blocks and items, or plain cubes only |
| `world_definitions` | Startup; immutable world definitions and optional startup instances |
| `on_player` | Ordered join, leave, world-change and sampled move observations |
| `on_place` | After player placement of an owned block |
| `on_break` | After player break; includes old ID and data |
| `on_interact` | Player uses an owned block; interaction is consumed immediately |
| `on_neighbor_changed` | A neighbor of an owned block changes |
| `client_message` | Actor's typed client record; valid focus snapshot or no block access |
| `epoch` | Client part changes world epoch; resend required state |
| `on_scheduled_update` | An owned block's scheduled update becomes due |
| `on_tick` | Each fixed tick of a bound world instance, with or without viewers |
| `on_timer` | A timer from `Callback::schedule` becomes due |

Block callbacks do not report changes from liquids, explosions or other plugins.
A fresh guest instance runs each server callback: memory does not survive it.
Use block data or scoped records for durable state. `callback-info.tick` supplies
the world tick; there is no ambient clock or randomness. A borrowed `Callback` expires at return.

## Reads, writes and commits

| Operation | Contract |
| --- | --- |
| Read block/state/data | Anchor, its six neighbors and network members, from a snapshot plus staged writes |
| `set_block` | Anchor/neighbor write scope in the anchor's chunk column; replaces air or owned blocks with air or owned blocks |
| `set_block_data`, `set_block_state` | Owned blocks in the write scope, plus snapshotted network members |
| `tell` | Event actor only; no control characters or formatting prefix |
| `send_client` | Actor's active client part on its declared channel; ordinary best-effort delivery after commit |
| `send_control` | Same recipient rules; retains negotiation/acknowledgment replies when transport credit is exhausted |
| `inventory`, `set_slot` | Actor's snapshot; writes are checked again at commit |
| `drop_item` | Fresh stack at a position where `set_block` may write |
| `crafting_result` | Matches the supplied grid against the host's crafting-table recipes |
| `schedule_update` | Owned block in data-write scope; survives restart, runs when loaded and still unchanged |

An accepted mutation is staged. A guest error or trap discards all gameplay output;
a refused individual operation stages nothing. Logs survive failures. The adapter
commits block/inventory results in one world transaction, after checking snapshot
tokens, written inventory slots and actor presence. Lifecycle operations run
outside that transaction. A stale result applies nothing.
Tells follow world mutations; client messages follow the world transaction.
A full block-event queue drops and counts new events without blocking the world
thread. A full player-observation queue suspends that Experience to avoid gaps.

Replacing a block, even with the same ID, clears its data and starts a new placement
generation. State changes retain data and generation. Missing data differs from empty data.
Unknown, unloaded and out-of-scope cells are not air; inspect returned errors.

Focus is the player's last interacted block for that Experience. It becomes invalid
on replacement, unload, dimension/world change or excessive distance. The current
distance uses the provisional `provisionalFocusRange` in the adapter's
[limits](../tools/localserver/experience/limits.go). It is incomplete parity work.

Network membership follows face-adjacent owned members. Break callbacks include
both sides of a split. Guests decide which faces actually connect. Flooding stops
at unloaded cells and network/data/frame budgets; treat a truncated network as
unavailable. Commit checks members and boundary changes.

## Worlds and player lifecycle

Server WIT 0.8 adds world definitions, player observations and scoped state.
The SDK defaults the new exports to empty/no-op implementations. Rebuilt guests
must declare `api = "0.9"`; existing 0.1–0.6 and 0.8 artifacts retain their frozen ABIs.

| Type or operation | Contract |
| --- | --- |
| `world_definitions` | Mod-namespaced immutable definitions; empty/flat generator, Overworld/Nether/End environment, persistent/temporary terrain |
| Definition, instance, handle | Distinct identities: a definition, stable storage ID, and one opening's generation; network dimension numbers stay in the host |
| Startup instances | Open before players join on every start; runtime-created instances reopen only on request |
| `Callback::world` | Current instance opening; legacy `info` metadata keeps its meaning |
| `resolve_world` | Snapshot lookup of this mod's open instances |
| `create_world` | Open an instance of an already registered definition |
| `unload_world` | Refuse occupied or reserved instances; invalidate handles before closing |
| `transfer` | Move only the callback actor to an authorized handle and finite position |
| `player_location` | Only the callback actor's live location; unavailable for leave |

Each lifecycle mutation must be the callback's only staged operation. Acceptance
means staged for host validation, not completed. Observe creation in a later
`resolve_world` and travel through `on_player`. Stale handles fail after unload,
reopen or server restart. World definitions cannot be added in callbacks.

Persistent terrain uses `<world>/experience-worlds/<mod>/<instance>/`. Reopening
retains terrain and private records but gives a new handle generation. Changing
a saved definition needs an explicit migration. Temporary terrain survives chunk
eviction, then disappears on close; its block records are removed and quota is
refunded. Fresh temporary openings also clear old block records after a crash.
Scoped records have independent lifetimes. Instances share the local gameplay
settings and pause state; new instances start at their generator's spawn, while
persistent reopenings retain saved spawn settings.

| `on_player` event | Contract |
| --- | --- |
| Join | Initial committed placement, before movement observations |
| World change | Historical before/after locations with handles and epochs |
| Move | First/last committed sampled positions; at most once per player per host tick, independent of pauseable world clocks |
| Leave | Historical location and private player-state access; no live inventory, position reads, messages or travel |

Join, move and world-change include inventory snapshots; writes recheck slots.
Historical locations remain readable after later travel; writes still need current
session/epoch authority. Departure clears focus and invalidates queued work, reads
and deferred sends, even after returning to the same live instance.

The host prepares the destination before detach. Failed admission restores the
source with a newer epoch, including cancellation after detach. A disconnected
player loses authority; cancellation after admission leaves the destination in
charge. Client traffic stays fenced until the correlated acknowledgment and any
preparation finish; see [wire fencing](server-experiences.md#versions-and-wire).
An active wire-v1 client part blocks coordinated departure; mandatory native moves
end that part. Initial placement can still activate v1. Ordinary players need no
extension notice. Failed notice delivery ends only the optional client part.
Acknowledgment does not prove terrain has rendered; live client-part transfer
qualification and crash-atomic multi-provider saves remain incomplete.

Player saves pair stable instance identity and position in `<world>/player-worlds/`.
Reconnect uses that instance's current opening. If closed or missing from an older
save, use the default Overworld spawn, without the other instance's coordinates.

## Ticks and timers

Server WIT 0.9 adds `on_tick` and `on_timer`; older artifacts receive neither.

| Rule | Contract |
| --- | --- |
| Clock | `SimulationStep` carries the counter and fixed tick duration (ns). Unload pauses it; reopening resumes with a new world generation. Process-local: not restored after a server restart |
| `Callback::schedule(after, period)` | Handle scoped to this mod and instance opening. Delay starts at commit; period 0 runs once, otherwise repeats. Unload discards the opening's timers |
| `cancel_schedule` | Cancels an active handle, including an occurrence already queued behind the caller |
| Final leave | No simulation context: scheduling and cancelling return `Unavailable`; private-state writes still work |
| Order | Per tick: `on_tick`, then due timers by handle, in the mod's FIFO. Callbacks see their admitted tick even if they run later |
| Overload | Queue overflow drops and counts ticks and repeating occurrences. A failed one-shot stays due. Repeating timers keep at most one queued occurrence; missed periods are skipped, never replayed |
| Limits | Active timers per mod and instance share `MAX_SCHEDULED_UPDATES`; delays use `MAX_SCHEDULE_TICKS` |

## Blocks and items

Paired declarations cover cubes and items; richer guests return `Registration`.
Blocks may declare states, placement traits, indexed geometry, material bindings,
flipbooks and ordered visual permutations. State names belong to the Experience;
placement-trait axes precede authored axes. The first value is the default.
Later matching permutations override earlier ones. A zero rotation cannot undo an
earlier nonzero rotation. Plain cubes have full collision/selection, are harvestable
by hand and drop themselves. See the WIT and [registration validator](../crates/experience-runtime/src/load.rs).

The actor's inventory has hotbar slots first and offhand last. Fresh stacks may
contain own-item data; foreign/ordinary stacks cannot. Vanilla metadata variants,
stack limits and offhand eligibility come from the adapter's item catalog.
Own-item data persists with the stack and reaches clients; decode it as untrusted.
Private block data does not enter chunk NBT.

## Budgets and limits

Use the constants below rather than treating a denied operation as success.
[Runtime limits](../crates/experience-runtime/src/limits.rs) are authoritative;
the adapter's mirrored commit limits are checked against generated fixtures.

| Resource | Source constants |
| --- | --- |
| Fuel and deadlines | `CALLBACK_FUEL`, `REGISTER_FUEL`, `CALLBACK_DEADLINE`, `REGISTER_DEADLINE` |
| Guest memory, stack, tables, instances | `MAX_MEMORY_BYTES`, `MAX_WASM_STACK_BYTES`, `MAX_TABLE_ELEMENTS`, `MAX_CORE_INSTANCES` |
| Artifact and declarations | `MAX_COMPONENT_BYTES`, `MAX_MANIFEST_BYTES`, `MAX_BLOCKS`, `MAX_ITEMS`, `MAX_STATE_BITS`, `MAX_PERMUTATIONS` |
| Callback work | `MAX_HOST_CALLS`, `MAX_STAGED_OPS`, `MAX_STAGED_DATA_BYTES`, `MAX_SCHEDULED_UPDATES`, `MAX_SCHEDULE_TICKS` |
| Private/item data | `MAX_BLOCK_DATA_BYTES` per block or scoped record, `MAX_ITEM_DATA_BYTES` |
| Messages and tells | `MAX_CLIENT_SENDS`, `MAX_CLIENT_SEND_BYTES`, `MAX_VALUE_DEPTH`, `MAX_TELLS`, `MAX_TELL_BYTES` |
| Networks and recipes | `MAX_NETWORK_BLOCKS`, `MAX_NETWORK_DATA_BYTES`, `MAX_RECIPES`, `MAX_RECIPE_INPUT_ITEMS` |
| IPC | `MAX_FRAME_BYTES` |
| Adapter lifecycle/storage | `loadDeadline`, `resultDeadline`, `eventQueueCap`, `dataQuota`, `flushInterval` in [adapter limits](../tools/localserver/experience/limits.go) |

`experience-runtime serve --report-fuel` logs callback fuel use. Client grants and
message/byte rates apply in addition to the server callback budgets; see
[client parts](server-experiences.md#permissions-and-budgets).

## Private data store

`<world>/experience-data/<id>.json` stores block data by stable world-instance ID
and position, with placement generations and data revisions, plus scoped records.
Orphaned block data is not returned. Dirty files flush atomically on the configured interval and shutdown.
A crash can lose one interval of writes; world saves and data saves are independent.

| Private record scope | Authorized identity |
| --- | --- |
| `StoreScope::Player` | Callback's stable player ID, including final leave; writes require session authority |
| `StoreScope::Instance` | Callback's current stable instance |
| `StoreScope::Mod` | Installing mod |

Each scope holds one versioned record per identity. `Persistent` and
`Callback::read_state`/`write_state` use serde JSON. `store_read`/`store_write`
expose bytes for explicit migrations: read the old schema before replacing it.
A different schema read fails; absent and empty differ, and `None` deletes.
Deleted records retain revisions. Records stay private unless explicitly sent.

Block and scoped writes validate and commit together, sharing `dataQuota` by net
byte change and the callback's staged-data/operation budgets. Snapshots contain
only authorized scopes. Leave writes retain scope/revision checks without a live
world. The flush crash window still applies.

Bind worlds with stable `cinnabarext.WorldID` values. Opening generations are
reserved in `_worlds.json` before publication, including across restarts.
Store schema 3 adds scoped records to schema 2's instance-keyed blocks; schema 1's
dimensions migrate to the three existing world instances. Unknown or ambiguous
legacy identities fail startup. Back up before upgrading: older servers require the old backup to downgrade.
`_installed.json` records required Experience and block IDs; removing either fails
startup. For an older ID-only manifest, restore original artifacts and explicitly
add a `blocks` map of each recorded Experience to its original block IDs.

## Reload

Server stdin accepts `pause`, `resume`, `experience reload <id>` and `stop`.
Pause holds dispatch and commits. Reload clears strikes, restarts and quarantine,
but must preserve ID, registered blocks/items in order and world declarations;
code and version may change. A failed reload leaves the Experience quarantined.

Guest traps, budget failures and protocol violations count as strikes; guest
rejections, host cancellation, transport and host encoding failures do not.
Unusable helpers are replaced; host-canceled replacements consume no fault
restart. The adapter's strike/restart windows can quarantine one Experience while its blocks and other Experiences remain.
Shutdown stops admission, delivers final leaves and drains callbacks before
flushing stores, reaping helpers and closing worlds. The local server uses a
ten-second drain deadline and reports timeout as an error; a full queue does not
drop final leaves. Custom hosts must call `Host.Drain` with a bounded context
before closing the server or worlds.

## Host protocol and versions

| Axis | Current contract |
| --- | --- |
| Server WIT | 0.9; frozen 0.1–0.6 and 0.8 worlds remain in [runtime WIT](../crates/experience-runtime/wit) |
| Adapter IPC | 9, `PROTOCOL_VERSION` in [protocol.rs](../crates/experience-runtime/src/protocol.rs); exact match required |
| Manifest data schema | `DATA_SCHEMA` in [artifact.rs](../crates/experience-sdk/src/artifact.rs) |
| Client WIT/wire | [Client compatibility](server-experiences.md#versions-and-wire); [shared drawing](server-experiences.md#drawing-and-signed-assets) |

Server 0.1 has no client messages; 0.2 adds scalar messages; 0.3 adds nested records
and epochs; 0.4 adds focus; 0.5 adds block states/visuals, items, inventories and
networks; 0.6 adds recipes and scheduled updates; 0.8 adds worlds, player events
and scoped state; 0.9 adds ticks and timers. Unsupported callbacks are rejected.

A host runs one `experience-runtime serve` helper per artifact over private stdio.
Frames are a little-endian u32 JSON length followed by JSON, bounded by
`MAX_FRAME_BYTES`; binary fields use lowercase hex. Clear the helper environment,
forward stderr separately, check `loaded`'s protocol, and register content before
freezing the server registry. Never run guest IPC on the world thread.
Snapshot events, validate and commit returned operations atomically, then deliver
messages on declared client routes. Own instance generations, player-session and
epoch fences, scoped snapshots, storage, stale checks, quotas, strikes and shutdown
deadlines. Current Go `OnClientMessage`/`OnEpoch` hooks receive the originating
`cinnabarext.PlayerSession`; match the admitted session before routing input. Use
the [Go adapter](../tools/localserver/experience) and its `testdata/protocol`
fixtures to check a new host.
