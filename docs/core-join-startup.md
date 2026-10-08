# Core join startup evidence

The initial outbound sequence is `RequestChunkRadius`, `ServerboundLoadingScreen(Start)`,
then, after local loading completes, `ServerboundLoadingScreen(End)` and
`SetLocalPlayerAsInitialized`. Receiving `PlayerSpawn` alone does not complete loading.
This documents control flow, not a captured retail wire trace.

## Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Radius first | StartGame directly requests and sends the chunk radius. |
| Loading transitions | Scheduled loading-screen transitions queue start/end packets. Start follows the direct radius request; end follows closing the screen. |
| Initialization | Send the local runtime ID once terrain loading has completed, the dimension is stable, and the player is in the world with no menu screen showing. This is local readiness after the screen closes, rather than a direct PlayerSpawn response. |
| Pack acquisition and selection | Check compatibility and the required bit of the selected stack; incompatible required content is fatal. Acquisition separately handles required and optional downloads. |
| Transfer | Pass the packet's address and port to the server-transfer initiator to build the destination connection. This does not establish a bridge shutdown algorithm or spawn prerequisite. |

The vanilla **1.26.50.4 resource pack** at
`full/resource_pack/texts/en_US.lang:8242`–`:8245` has distinct optional, required
and server-required download prompts. `ui/progress_screen.json:1209` defines the
world-loading screen independently. These files do not establish packet order.
`texts/en_US.lang:1546` describes transfer as moving a player to another server,
and `:8237` labels the transfer screen “Loading World”.

## Implementation and regression coverage

The Rust login sends radius before loading-start and returns the stream once the server's
spawn prerequisites arrive. It retains a pending runtime ID. The app validates and installs
packs, waits for its terrain presentation gate, closes its loading screen, then queues one
completion command. That command sends loading-end followed by initialized once.

A server may send no terrain until it receives initialized: the pinned Dragonfly blocks in
`conn.StartGameContext` (`server/server.go` `finaliseConn`) until `SetLocalPlayerAsInitialised`
and only then adds the player and streams chunks, answering the radius request with
`ChunkRadiusUpdated` and `PlayerSpawn` alone. A probe sending Cinnabar's order to the local
server received 0 chunks before initialized and 637 in the 8 s after. Jolyne therefore records
whether a publisher update, level chunk or sub-chunk preceded spawn. When none did, the startup
view is empty until the server publishes one, so the gate releases once received work drains.
**Provisional:** vanilla must also complete loading without terrain here (it joins Dragonfly),
but the path that does so remains unresolved. Vanilla's terrain loading step
completes only once loaded chunks reach the needed count or, after a deadline,
every chunk in its fixed nine-column load set is loaded, and a position check
passes. The following loading step normally completes directly; an explicit stop
also completes terrain loading, through callers not resolved here.

`offline_core_preserves_spawn_order_and_startup_transfer` starts the production Go relay
against a local scripted upstream and runs the actual Rust socket login. The upstream
asserts every relevant outbound packet in order, with a round-trip barrier proving that
completion cannot precede explicit presentation readiness. A second completion call must
not send duplicates. The transfer cases send StartGame then Transfer, in separate frames or
one batch, and immediately close without supplying spawn prerequisites. A connection-context
barrier waits for upstream EOF before starting either relay pump; no sleep controls the race.
The destination reaches Rust as a typed terminal event, which the app routes to its existing
reconnect owner.

The relay drains queued upstream batches before teardown when the reverse writer observes
an ordinary upstream close. Rust also reads terminal input if its startup response write
fails: Transfer or Disconnect takes precedence over that write failure. Without either
terminal packet it retains the original write error; the existing startup deadline and
owner cancellation still bound the drain. Scripted tests force write failure with both batch
layouts, and relay tests cover queued delivery and cancellation.

This verifies the startup packet contract. It does not certify all terrain-readiness
thresholds, dimension transitions, consent dialogs, or live-server/visual parity. The existing
terrain presentation gate remains the app's readiness criterion; full parity remains open.

## Near terrain can complete before distant requests

The presentation gate also accepts the loaded 3×3 neighborhood around the server's
player position once its resident sections have current light and acknowledged
meshes. It still requires a later GPU-completed frame. Far replies and their
lighting/meshing queues can continue after the loading screen closes. This avoids
requiring a fully drained view when the visible opaque count is below the dense
threshold. `fa7af5ed` connected the initialization notification to that older gate;
it is not proof that the BDS in the supplied screenshot withheld replies.

Vanilla tests the spawn neighborhood when the full view is incomplete. Nine
`ChunkPos` offsets cover x/z −1 through 1. Loading completion leads to the
initialization notification; the notification worker is separate from the
neighborhood readiness calculation.

- Vanilla pack `ui/progress_screen.json:1215` and
  `texts/en_US.lang:8153`, `:8179` supply the retained loading presentation.

`bds_local_startup_completes_with_distant_replies_withheld` runs real requests,
decoding, lighting, meshing and upload acknowledgements. Near terrain becomes
ready while a distant column remains withheld and the old drained predicate is
false (725 ms in the focused run). Releasing that reply still drains the stream.
`local_terrain_releases_after_a_gpu_frame_with_distant_work_pending` checks the
additional presentation fence. `bds_join_dense_columns_drain_with_a_stationary_camera`
checks full-height terrain; `bds_saved_terrain_drains_without_camera_motion` can
replay local occupancy records through `CINNABAR_BDS_TERRAIN`.

The full-height dense fixture drained in 5.71 s at the harness's normal 8 ms
frame cadence, with 495 visible meshes and no pending light or mesh jobs. The
accelerated 1 ms fixture previously exhausted its frame count under concurrent
gate load; the convergence test now keeps wall time closer to its simulated
reply clock. Earlier accelerated runs took 7.37 s before and 6.41 s afterward;
these mixed-load observations are not a pipeline speedup claim. An occupancy
replay extracted read-only from the restored BDS world drained in 1.25–2.94 s. Neither reproduces the reported minutes
or 7 FPS, and occupancy is not an exact packet capture. The restored client-tail
log is empty. These offline results do not establish a live BDS join time.
