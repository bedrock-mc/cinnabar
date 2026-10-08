# Server Experiences

A server Experience is a WebAssembly guest that adds stateless cube blocks to the local Dragonfly
server (`tools/localserver`) and reacts to them being placed, broken, used and disturbed. Each
Experience runs in its own `experience-runtime` helper process, off the world goroutine; the Go
adapter in `tools/localserver/experience` validates what the guest asks for and commits it.

This file is the canonical description of the runtime's semantics. Numbers live in the sources it
names, never here.

## Running

```text
bedrock-local-server -dir <world> -addr <addr> -experiences <dir> -experience-runtime <binary>
```

`make local-server` builds both `bedrock-local-server` and `experience-runtime`
(`cargo build -p experience-runtime --release --locked`). `-experience-runtime` is required with
`-experiences`. Without `-experiences`, startup checks any existing installation manifest and
refuses a world that requires Experiences. Fresh worlds create no Experience files and spawn no
helper. The `-extension-*` flags add the server half of client parts; see
[Client parts](#client-parts).

Startup, all before the server listens and prints `ready`; any failure exits with an error:

1. Every immediate subdirectory of `-experiences` that contains `experience.toml` is an artifact,
   taken in byte order of the directory names.
2. The private store opens at `<world>/experience-data`.
3. One helper per artifact starts and loads it. Two artifacts with the same id fail startup.
4. Every recorded Experience and block ID must still be provided. A missing definition fails
   startup before the world opens. Additions, ordering and presentation changes are allowed.
5. The blocks are registered with Dragonfly before `server.Config.New`, which builds the
   resource pack from them, each Experience getting a creative group named after its id. The
   started ids are then recorded as installed.

Stdin commands, besides `pause`, `resume` and `stop`:

- `pause` and `resume` also hold and release Experience dispatch and commits.
- `experience reload <id>` reloads that Experience (see
  [Strikes, quarantine and reload](#strikes-quarantine-and-reload)) and logs the result on
  stderr.
- `stop`, stdin EOF, SIGINT and SIGTERM close the server first, then the Experiences: admission
  stops, in-flight work is cancelled, the store is flushed and the helpers are shut down.

On Windows the helpers share the server's console, so Ctrl+C in that console reaches them as well
as the server, which shuts down on it anyway. A helper that Ctrl+C ends before the adapter's
shutdown frame may show up in the shutdown error or as a fault in the log.

## Artifact

An artifact is a directory holding `experience.toml`, `server.wasm` and `assets/`. One
`experience.toml` declares both halves of the Experience:

```toml
id = "benergistics"   # owns the block namespace "benergistics:"
version = "0.1.0"     # of both halves; the client part's package_version
api = "0.3"           # server WIT major.minor
data-schema = 1       # block-data schema

[client]              # the client part; the runtime ignores this table
permissions = ["ui", "messaging"]
actions = []          # declared action ids
templates = []        # ui/<name>.json files, read beside experience.toml
textures = []         # files under textures/, read beside experience.toml

[[client.channels]]
id = "benergistics.controller"
schema = 1
direction = "to_client"
fields = [{ type = "integer", min = 0, max = 4294967295 }]

[files]               # every other file, '/'-separated, with its lowercase hex SHA-256
"server.wasm" = "…"
"assets/controller.png" = "…"
```

`crates/experience-runtime/src/manifest.rs` is the authority on the manifest: the id and version
rules, the accepted `api` (each server WIT version the runtime implements, see
[Version matrix](#version-matrix)) and `data-schema`, and the index rules (every file indexed,
no absolute paths, `..`, backslashes or symlinks). The hashes give integrity, not publisher
trust. The id `minecraft` is reserved: it is the namespace of vanilla blocks, so the adapter
refuses an Experience with that id at registration, before anything is registered.

The runtime ignores `[client]`, whatever it holds, and refuses any other unknown key. The server
half of client parts routes messages by the client part's signed manifest, never by this file,
and `cinnabar-cxb build --experience` checks the table with the client's own verifier when it
signs it into the `.cxb` ([Client parts](#client-parts)). The Go adapter only looks for the file
and never parses it. A channel declaration uses the signed manifest's own spelling
(`to_client`, `max_bytes`, `max_items`; see
[server-experiences.md](server-experiences.md#typed-runtime-messaging-and-publication)).

`server.wasm` is the core module that cargo emits for `wasm32-unknown-unknown` with the WIT
embedded by `experience-sdk`. The runtime componentizes it with `wit_component::ComponentEncoder`
(the route `mod-host` uses) and instantiates it against the exact `server` world of its manifest's
`api`. A client component, a WASI import, another `api`'s world or any unknown import fails the
load; serialized native Wasmtime artifacts are never accepted.

### The author SDK

`crates/experience-sdk` is one SDK for both halves, each its own component:

| Feature | Module | Builds |
|---|---|---|
| `server` | `experience_sdk::server` | the server half: implement `Experience`, export it with `export_experience!`; WIT in `wit/server/` |
| `client` | `experience_sdk::client` | the client part: implement `ClientPart`, export it with `export_client_part!`, send with `client::send`; WIT in `wit/client/` |
| `declarations` | `experience_sdk::declarations` | for a build script: `generate("<path>/experience.toml")` |

Both halves share `Value`, one value of a channel record, and the channel declaration types
`Channel`, `Direction` and `Field`. The server half passes records as `server::nodes` and
`server::values`; the client part receives and sends them as `Value`s, which the SDK carries in
the wire's JSON form. Every feature is on by default, so the SDK's own tests and docs cover all
of it; a crate that uses the SDK sets `default-features = false` and names what it uses.

A guest takes its channels, actions and templates from `experience.toml`, so neither half
restates one: its build script, with the SDK as a build dependency with `declarations`, calls
`experience_sdk::declarations::generate`, and `mod experience { experience_sdk::include_declarations!(); }`
brings in `channels` (a `Channel` constant per `[[client.channels]]`, named after its id without
the `<id>.` prefix in upper snake case), `actions` and `templates` (`&str` constants). Two
declarations with one name, a channel outside the namespace and a template that is not
`ui/<name>.json` fail the build.

### Building and packaging

1. Write `experience.toml` without `[files]`.
2. Write the server half: a `cdylib` crate that depends on `crates/experience-sdk` with the
   `server` feature and takes `experience.toml`'s declarations. `examples/experiences/probe` is a
   complete example.
3. Write the client part, if there is one: a `cdylib` crate with the `client` feature.
4. `cargo build -p <crate> --target wasm32-unknown-unknown --release --locked` for each.
5. Copy the server module to `server.wasm` and the textures under `assets/`, and write
   `experience.toml` with `[files]` appended, listing the SHA-256 of every other file.
6. `cinnabar-cxb build --experience experience.toml --component <client part .wasm>
   --publisher-seed <seed file> --out <id>.cxb` signs the client part.

The Applied Benergistics repository's `scripts/package.ps1` (and `scripts/package.sh`) does all of
this from its root `experience.toml`, and verifies the result; `-VerifyOnly` checks an existing
package.

The runtime verifies every `[files]` hash when it loads the artifact. The adapter reads the
texture files afterwards, when it registers the blocks, so an operator who edits an artifact while
the server starts can get textures that were not the hashed ones. Do not change artifacts during
startup.

## WIT and semantics

The contract is `crates/experience-sdk/wit/server/server.wit`, package
`cinnabar:experience-server@0.4.0`, world `server`. The guest exports `register`, which runs once
at startup and declares its blocks, the callbacks `on-place`, `on-break`, `on-interact` and
`on-neighbor-changed`, `client-message` and `epoch`. Every world method goes through the borrowed
`callback` resource, valid for one callback only. The runtime still runs older artifacts against
their frozen worlds, by the manifest's `api`:

- `api = "0.3"`, `crates/experience-runtime/wit/0.3/server.wit`: no `callback.focus`, so client
  messages and epochs never have a snapshot. The adapter gives such an Experience no focus, and
  the runtime rejects one with a focus without running it.
- `api = "0.2"`, `crates/experience-runtime/wit/0.2/server.wit`: client messages and sends hold
  scalars only, and there is no `epoch`. A client message holding a list or record, or an epoch,
  for such an Experience is rejected without running it.
- `api = "0.1"`, `crates/experience-runtime/wit/0.1/server.wit`: neither `send-client` nor
  `client-message` nor `epoch`; a client message or an epoch for it is rejected without running
  it.

WIT cannot express the rules below; the runtime (`crates/experience-runtime`) and the adapter
(`tools/localserver/experience`) both enforce them.

- **Blocks.** Stateless cubes registered at startup only: an opaque texture per material slot
  (`*` or all six faces), full-cube collision and selection, and mining that is either
  `unbreakable` or `breakable(hardness)`. Every block is harvestable by hand and drops itself.
  Block ids are `<experience id>:<name>`.
- **Callbacks.**
  - Place: after a successful player placement.
  - Break: after a player break, with the old id and the old data.
  - Interact: the server consumes the interaction at once and queues the call.
  - Neighbor: queued when a block next to one of the Experience's blocks changes.
  - Liquids, explosions and other plugins produce no notification.
- **Staging.** An `ok` from a mutation means staged. A guest error or a trap discards everything
  staged; a rejected single operation leaves the staged state unchanged and the callback
  continues. Logs are not gameplay output and survive a discarded callback.
- **Reads.** Reads see only the anchor (the event's block, or a client message's or an epoch's
  focus) and its six orthogonal neighbors in the same dimension, as they were snapshotted.
  - `get-block` returns a snapshot position's id with staged writes applied; an unloaded position
    is `unavailable`, one outside the snapshot `denied`, one outside the world height
    `out-of-bounds`.
  - `block-data` returns data only while the position holds this Experience's block (staged
    writes applied), else `not-owned`. No data and empty data are distinct.
- **Writes.** Writes reach the anchor and the snapshot neighbors in the anchor's chunk column.
  - `set-block`: the current block (staged writes applied) must be air or this Experience's own,
    else `not-owned`; the new id must be `minecraft:air` or this Experience's own, else
    `unknown-block`. Every replacement, even with the same id, clears the position's data and
    starts a new block generation.
  - `set-block-data`: only on this Experience's own block in the write scope. Data over
    `MAX_BLOCK_DATA_BYTES` is `too-large`; data over the Experience's remaining budget, which
    each callback carries, is `quota-exceeded`.
- **`tell`.** Only to the event's actor, else `denied`; `player-unavailable` when the event has no
  actor. Control characters and `§` are `invalid-text`; text over `MAX_TELL_BYTES` is
  `too-large`; at most `MAX_TELLS` per callback.
- **`send-client`.** Stages a typed record (the client wire protocol's field values) for the
  actor's client part on a channel and schema revision; `denied` and `player-unavailable` as for
  `tell`. A callback's channels and payloads hold at most `MAX_CLIENT_SEND_BYTES` as JSON, else
  `too-large`; that is room for one message as large as wire v2 carries, on a channel with the
  longest id, which `TestRuntimeClientSendsFitTheWire` in `tools/localserver` checks against the
  wire's constants. At most `MAX_CLIENT_SENDS` per callback. The adapter sends a staged message
  only after the whole result commits, and only on a channel, in the direction to the client,
  that the Experience's own client part declares; anything else, or a player without an active
  client part, is dropped and counted. Without the server half of client parts every staged
  message is dropped.
- **Payload values.** A record's values are scalars, lists and records, as the channel declares
  them. WIT has no recursive types, so a payload is a `list<value-node>`: its values in
  pre-order, a scalar as a `leaf`, a list or record as a header holding its item count followed
  by that many values. That is the layout of MessagePack and CBOR arrays, and the boring choice:
  unlike a node table with child indices it cannot share a node or form a cycle, so the only
  malformed input is a header that counts more items than follow it, and unlike JSON text the
  leaves stay typed and neither side runs a parser. Lists and records nest at most
  `MAX_VALUE_DEPTH` deep, a top-level one being level 1, which is the wire's `MAX_FIELD_DEPTH`;
  a deeper payload is `too-large`. A malformed payload traps. The SDK's `Value`, `server::nodes`
  and `server::values` build and read payloads, and `Experience::client_message` receives
  `Value`s.
- **`client-message`.** A typed record that a player's client part sent arrives through the same
  queue as the block callbacks, with that player as the actor, its lists and records as pre-order
  nodes like a send's. Its `callback` has the snapshot of the player's focus, if any; without one
  every block read and write is refused. It may `tell` and `send-client` to the player, and its
  result commits like any other, in the world the player is in when it runs.
- **`epoch`.** A player's client part moved to a new world epoch, such as another dimension, and
  kept running, so it may have missed what was sent before; the guest resends its state. The
  callback comes through the same queue, with that player as the actor and the snapshot of the
  player's focus, and acts exactly like `client-message`'s.
- **Focus.** When a player's `on-interact` runs for one of an Experience's blocks, the adapter
  records that block, its dimension and its placement generation as the player's focus for that
  Experience; a newer interaction replaces it, and a disconnect clears it. A `client-message` or
  `epoch` of that player gets exactly the snapshot `on-interact` gets for the focus block: the
  block and its six neighbors, read and written by the same rules, writes in its chunk column.
  `callback.focus` names the block, and is none in every other callback. The focus counts as
  none, an empty snapshot, while the block is gone or replaced (another generation), no longer
  this Experience's, unloaded, in another dimension than the player, or farther than
  `provisionalFocusRange` (`tools/localserver/experience/limits.go`) from the player's eyes to
  the block's centre. That bound is provisional, labeled incomplete in `plan.md`: it should be
  the distance at which vanilla Bedrock closes an open container's screen, the player's pick
  range (per input mode, survival or creative) from the eyes to the block's centre. Those
  constants are not yet known, so it is Dragonfly's survival reach for using a block. Commit and the
  stale check are unchanged: if the focus block or its data changed after the snapshot, the
  result is discarded.
- **No ambient time or randomness.** `callback-info.tick` is the integer world tick.
- **Fresh instance per callback.** Each callback runs on a new instance of the precompiled
  component, so guest memory never survives a callback; durable state belongs in block data.
- **Commit.** The adapter commits a result in a fresh world task, whole or not at all. If any
  snapshotted block or data changed meanwhile, or the actor is no longer connected in that
  world, the result is discarded as stale. Otherwise block and data ops apply, then the tells,
  and then, outside the world task, the client messages.
- **Guest imports never call back into the live world.** Hooks on the world goroutine only
  enqueue; a full queue drops the event and counts it.

## Client parts

The server half of client parts (`tools/localserver/extension`) offers each Experience's client
part, a `.cxb` that `cinnabar-cxb build --experience` makes from the Experience's
`experience.toml`, over PR #34's unchanged handshake and typed channels
([server-experiences.md](server-experiences.md)). On the client a failed client part callback
drops only its own output and the helper restarts the guest; repeated failures stop that part,
as the server stops a failing Experience (see server-experiences.md on helper failures).

```text
bedrock-local-server … -extension-key <seed file> -extension-audience <host:port> -extension-cxb <dir>
```

- **Flags.** The three come together. `-extension-key` is a raw Ed25519 seed as
  `cinnabar-cxb keygen` writes it; keep it under `.local/`. `-extension-audience` is the address
  players join by, exactly as the client canonicalizes it: lowercase host, bracketed IPv6 and an
  explicit nonzero port, such as `127.0.0.1:19132`; a client that joins by another address
  ignores the offer. `-extension-cxb` is a directory whose `.cxb` files, in byte order of their
  names, are offered. A bundle's id is the id of the Experience it belongs to.
- **Startup**, before the resource packs load; any failure exits with an error. Each bundle's
  manifest must be signed by the publisher key it names, at the client's API, with its channels in
  its namespace. The revision in `<world>/extension-revision` goes up by one and is stored first; a
  file that holds no revision fails startup instead of offering a lower one, which clients that
  pinned the server would refuse. The server key signs an offer of the bundles: the union of their
  permissions, the origin `https://cxb.invalid`, as much memory as the client gives that many
  guests, no GPU memory and a fixed fallback text. `.invalid` never resolves, so a client takes a
  bundle only from its cache, which `cinnabar-cxb seed-cache` fills. The offer expires a day less
  an hour after startup, the hour being margin for client clocks behind the server's; after that
  no joining player is offered client parts, so restart the server at least daily. The marker is
  written into the optional resource pack `<world>/resources/cinnabar-extension-offer`, whose
  version is the revision, so a pack cache never serves an older offer. Without the flags that
  pack is removed and nothing else changes.
- **Carrier.** Each connection that Dragonfly's RakNet listener accepts is wrapped. The wrapper
  takes `ScriptMessage`s with the identifier `cinnabar:extensions/v1` out of the packet stream and
  passes every other packet through untouched. Dragonfly's listener does not expose packet
  headers; it admits no sub-client login, so every packet on a connection is its primary
  client's, and Hello and envelopes must name sub-client 0.
- **Handshake**, per connection. A Hello must have the client's handshake and API versions, the
  current offer's digest, sub-client 0, 32-byte nonces and a capability set that is not empty,
  before the offer expires. The server answers with an Accept signed by the server key that
  echoes the Hello, with a fresh challenge and session, expiring with the offer. The Accept
  selects the highest wire version that the Hello's `wire` lists and the server speaks, at the
  lower of each of the Hello's ceilings and the server's (`HostLimits`, the client's own
  constants); a v1 Hello, or one whose highest common version is 1, gets an Accept that selects
  nothing, exactly as before wire v2. A Hello with no common version or unusable ceilings falls
  back. Ready must name that session, the offer's bundle digests in order and generation 1, and
  grant each bundle permissions within its manifest, the Hello's capabilities and the offer's
  scope. The client part is then active with Ready's world epoch.
- **Messages.** A committed `send-client` goes out only to an active client part, on a declared
  `to_client` channel of a bundle granted `messaging`, sequenced from 1 with the current world
  epoch and within the client's message and byte rates; anything else is dropped and counted. On
  wire v2 a record over the inline limit goes out in fragments, all of them or, when the client's
  remaining rate cannot take them all, none. Inbound envelopes, and on v2 fragments, pass the
  client's own ingress rules (route, sequence, order, bundle, namespace, `messaging` grant,
  schema, size, reassembly budget, rate) against the manifest's `to_server` channels and become
  `client-message` callbacks of the Experience whose id is the bundle id. An undeclared channel
  schema, which the client would skip as a newer revision, is a violation here, since the server
  knows the exact manifest; so is another world epoch on wire v1.
- **World epochs.** On wire v2 a dimension change keeps the client part. The client's `epoch`
  control moves the session to its new epoch: the server's later envelopes carry it, the client's
  envelopes of the old epoch are dropped and counted, and `Server.OnEpoch` queues the `epoch`
  callback (`Host.DeliverEpoch`) of each Experience with an active client part for that player,
  so it can resend its state.
- **Fallback.** Any violation, the Accept's expiry, a dimension change on wire v1 (it resets the
  client's world epoch) or a disconnect puts that connection in fallback for good: its client
  part gets nothing more, its messages are dropped, and the player stays connected and plays on
  as without a client part. A dimension change before the Hello ends nothing. One log line
  reports each client part that becomes active, changes epoch or falls back.
- **Tests.** `go test ./extension` covers the handshake rules, the v1/v2 negotiation matrix,
  epochs and fragment attacks with Dragonfly's listener faked; `TestClientPartHandshakeOverRakNet`
  runs a Hello over real RakNet. `testdata/go` holds the server half's own marker, v2 Accept,
  envelope and fragments, which `tools/cxb/tests/fixtures.rs` runs through the client's
  verifiers; regenerate them with
  `go test ./extension -run TestGoFixturesAreCurrent -update-go-fixtures`.

### Client part media (developer)

`-extension-media <dir>` (with the `-extension` flags) serves `<dir>` over HTTPS at
`-extension-media-addr` (IPv4 loopback, default `127.0.0.1:19443`), adds that origin and surface
GPU memory to the offer, and writes a fresh CA to `<world>/extension-media-ca.pem`. The client
fetches loopback media only with `CINNABAR_DEV_SERVER_EXPERIENCES=1` and `CINNABAR_DEV_MEDIA_CA`
naming that file. `cinnabar-cxb media` strips tags from a profile WebM and writes its descriptor;
`cinnabar-cxb build --assets <dir>` indexes it into a bundle.

## Limits

- `crates/experience-runtime/src/limits.rs` is the only source of the guest limits: fuel, epoch
  and wall deadlines, memory, tables, instances, stack, component and manifest size, blocks per
  Experience, host calls, staged ops and data, tells, client messages, block data, logs and the
  IPC frame.
- `tools/localserver/experience/limits.go` holds the adapter's limits: load and result deadlines,
  strikes and restarts, shutdown grace, queue and neighbor caps, flush interval, data quota and
  texture limits. The values that the commit check enforces again mirror Rust constants;
  `TestFrameLimitMatchesRust` and `TestCommitLimitsMatchRust` compare them with
  `testdata/protocol/limits.json`, which the runtime writes.
- `experience-runtime serve --report-fuel` logs the fuel each callback used.

## Adapter protocol

The adapter and a helper talk over the helper's stdin and stdout; the helper logs to stderr, which
the adapter logs tagged with the Experience id. A frame is a 4-byte little-endian length and that
many bytes of JSON, at most `MAX_FRAME_BYTES`; bytes inside messages are lowercase hex.

`crates/experience-runtime/src/protocol.rs` defines every message and `PROTOCOL_VERSION`.
`tools/localserver/experience/protocol.go` mirrors them. The golden fixtures in
`tools/localserver/experience/testdata/protocol` come from
`experience-runtime write-fixtures <dir>`; the Go tests decode and re-encode each one and require
identical JSON, and reject unknown fields. A helper that answers `load` with another protocol
version fails the load. A client message is a `callback` whose `call` is `client_message`, and an
epoch one whose `call` is `epoch`; both carry `focus`, the player's focus block or null, and
have that block's snapshot with a focus and an empty one without. `loaded` carries `focus`,
whether the Experience's world takes one; the adapter gives none to an Experience whose world
does not. A staged client message is a
`send_client` op. Their `scalar` values have the client wire protocol's form,
`{"type": "integer", "value": 42}`, a list or record holding its values in an array,
`{"type": "list", "value": [...]}`; the runtime turns them into and out of the guest's pre-order
nodes.

## Private data store

Block data never enters chunk NBT, so it never reaches clients. The adapter keeps it in
`<world>/experience-data/<id>.json`, one file per Experience, keyed by dimension and position;
each entry has a placement generation, a data revision and optional bytes. A placement or
replacement starts a new generation without data; a write bumps the revision. Reads check that the
live block is still the Experience's, so orphaned data is never returned. Each Experience may
store at most `dataQuota` bytes (`limits.go`).

Files are written atomically (temporary file and rename) every `flushInterval` (`limits.go`) when
dirty, and on shutdown. **Crash window:** a crash loses up to one flush interval of data writes.
The world and the store are saved independently, so after a crash a block can exist without its
latest data.

`_installed.json` records installed Experience IDs and their required block IDs together. Removing
an artifact or one of its block definitions fails startup. Both lists are replaced atomically.

An older ID-only manifest requires explicit migration: restore the original installed artifacts,
then add a `"blocks"` object mapping each recorded Experience ID to all of its original block IDs
(for example, `"blocks": {"probe": ["probe:counter"]}`). Do not infer these IDs from an updated
artifact that may have removed definitions. Startup refuses an incomplete manifest instead of
risking unreadable saved chunks.

## Strikes, quarantine and reload

The supervisor of each Experience (`supervisor.go`) runs one callback at a time within the result
deadline. A missed deadline, garbage, an oversized frame or a helper exit is a fault: the helper is
killed, reaped and restarted. `failed` results (trap, fuel, deadline, limit) and faults are
strikes; a guest's `rejected` error is not. Too many strikes or restarts within their windows
(`limits.go`) quarantine the Experience: its events are dropped without IPC, while its blocks stay
registered and in the world, and players and other Experiences carry on.

`experience reload <id>` starts a fresh helper and clears the strikes, the restart history and the
quarantine. The fresh helper must load the same id and the same block definitions, in the same
order, as at startup, because those are registered; only the version may change, so a reload can
ship fixed guest code. A failed reload leaves the Experience quarantined.

## Security: developer profile

This is a developer profile, not a sandbox for untrusted code. Guests are confined by Wasmtime and
the limits above, and helpers start with a cleared environment and private pipes, but helpers run
with the server's OS user and privileges and have no OS-level sandbox. Artifact hashes give
integrity, not publisher trust; artifacts are unsigned. Install only artifacts that you trust as
much as the server binary. OS-level restriction of the helper is a later, separate milestone.

## Version matrix

These axes are versioned separately. Before 1.0, a breaking change bumps the minor version.

| Axis | Version | Source |
|---|---|---|
| Server WIT | 0.3; 0.2 and 0.1 still accepted | `crates/experience-sdk/wit/server/server.wit`; 0.2 and 0.1 in `crates/experience-runtime/wit/<version>/server.wit` |
| IPC protocol | 3 | `PROTOCOL_VERSION` in `crates/experience-runtime/src/protocol.rs` |
| Server manifest | `api`, `data-schema`; `[client]` is ignored | `crates/experience-runtime/src/manifest.rs` |
| Client WIT | `cinnabar:server-experience@1.1.0`; 1.0 components still link | `crates/experience-sdk/wit/client/deps/server-experience/capabilities.wit`, world `server-bundle` in `crates/experience-sdk/wit/client/client.wit` |
| Client wire protocol | 2, negotiated in Hello and Accept; 1 still accepted | `WIRE_VERSION`, `MAX_WIRE_VERSION` in `crates/server-experience/src/policy.rs` |
| Bedrock target | | `assets/bedrock-target.json` |

A server artifact, its Bedrock art and a client extension are separate artifacts with separate
authority; accepting the resource pack is never consent to client code.

## Writing another adapter

Another server can host the same artifacts by speaking the protocol to `experience-runtime serve`:

1. Spawn one helper per artifact with a cleared environment and private pipes; forward its stderr
   to your log.
2. Send `load { dir }` within the load deadline and read `loaded` (or `load_failed`, after which
   the helper exits). Check its protocol version, register its blocks before your block registry
   freezes, and decode the textures it names.
3. For each event, snapshot the anchor and its neighbors with each cell's id, ownership, data and
   a token, send `callback`, and wait for `result` with the same `seq` outside your world thread.
4. Commit a `committed` result in one world transaction, whole or not at all, after checking that
   no token or block id changed and that the actor is still there; enforce the write scope, the
   ownership rules and the commit limits again, since the helper is not trusted.
5. After the commit, send each `send_client` op to the actor's client part if that Experience's
   client part declares the channel; drop and count the rest. Deliver a client part's messages
   as `client_message` callbacks, and its moves to a new world epoch as `epoch` callbacks, with
   that player as actor. If `loaded` said the Experience takes a focus, remember the block of
   each player's last interaction with it, and while that block is still valid give its snapshot
   and `focus`; otherwise send an empty snapshot and a null `focus`.
6. Count `failed` results and helper faults as strikes, and restart, quarantine and reload as
   described above.
7. Own the store: generations, revisions, the quota and atomic flushes.
8. Send `shutdown` on exit and kill a helper that outlives the grace period.

Check your implementation against the fixtures in `tools/localserver/experience/testdata/protocol`
and the Go adapter's tests.
