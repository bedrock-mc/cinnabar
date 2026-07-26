# Vanilla blob-cache and chunk-ordering reference for Bedrock 1.26.30

## Evidence boundary and provenance

This document records authoritative vanilla behavior observed in the Minecraft
Bedrock 1.26.30 client binary with its debug symbols. The observations were
contributed by the repository owner. Symbol names and relative virtual addresses
(RVAs) are included only so that a future investigator can re-verify the
observations. No disassembly, proprietary source, or copied code structure is
reproduced here.

Unless a program-wide or `.text` scan is stated explicitly, each observation is
attributed to the named symbol and RVA. The sections through "Cache status under
pressure" describe vanilla behavior. "Cinnabar divergences and open decisions"
describes the implementation's current differences separately.

## World-change ordering uses receive-side pause bookkeeping

Vanilla buffers selected world-change packets by pausing a per-connection
receive-side bucket. It does not use a per-column apply barrier.

Seven packet types route through
`ClientNetworkHandler::queueHandleWorldChangePacket` (RVA `0x0795c5a0`,
vtable slot `+0x7f0`):

- `UpdateBlockPacket`
- `UpdateBlockSyncedPacket`
- `UpdateSubChunkBlocksPacket`
- `BlockActorDataPacket`
- `BlockEventPacket`
- `ContainerOpenPacket`
- `LabTablePacket`

This list is exhaustive. The repository owner established it by scanning all of
the client's `.text` section for `call [reg+0x7f0]`, finding 31 call sites, and
resolving every hit.

On receipt of a `LevelChunkPacket`, vanilla increments
`mPendingChunks[{&dimension, ChunkPos}]` (RVA `0x7998db5`). The stored value is a
reference count, not a flag.

When the count for the packet's column is non-zero,
`queueHandleWorldChangePacket` calls
`NetworkSystem::setConnectionChannelPaused(id, 0, true)` to pause receive-side
bucket 0 and stashes a
`std::function<void(BlockSource&)>` in `mConnectionPausedCallbacks`. The
corresponding client log string is
`"Network Stream Paused for LevelChunk handling"`.

The paused packets are buffered rather than dropped.
`NetworkConnection::mPausedPackets` is a
`std::array<std::vector<PausedPacket>, 2>` at offset `+0x178`. On unpause, the
packets are replayed through `mResumedPackets`.

`ClientNetworkHandler::onChunkHandleCompleted` (RVA `0x0795da00`) decrements
the pending count. Only when it reaches zero does vanilla run the stashed
callback, erase it, and unpause bucket 0. The corresponding log string is
`"Network Stream Resumed after LevelChunk handling"`.

## The pause is recoverable because miss responses bypass buffering

`PacketHeader` is a 4-byte POD with one field, the `uint32` `mHeaderData`.
`PacketHeader::getChannel()` (RVA `0x0ac13a30`) classifies a received packet
into bucket 1 when the 10-bit packet ID (`mHeaderData & 0x3ff`) is 136
(`0x88`, `ClientCacheMissResponsePacket`), and into bucket 0 otherwise.

A program-wide caller scan found exactly one caller of `getChannel()`. Its
return value is used solely as an index into
`NetworkConnection::mPausedChannels`, a `std::bitset<2>` at offset `+0x148`,
and `NetworkConnection::mPausedPackets`, a
`std::array<std::vector<PausedPacket>, 2>` at offset `+0x178`. This is
receive-side pause bookkeeping, not a RakNet ordering channel or any other
transport channel; it does not cause any packet to be transmitted
differently.

While bucket 0 is paused, received packets classified into it are buffered,
but packet ID 136 is classified into bucket 1 and processed immediately.
That exception allows a client stalled on a column to receive the blob
payloads required to complete that column and unpause.

## Vanilla has no timeout on the pause

A program-wide caller scan found exactly two callers of
`setConnectionChannelPaused`: the pause in
`queueHandleWorldChangePacket` and the unpause in `onChunkHandleCompleted`.
No watchdog or timeout call site exists for this pause.

Consequently, if a server never answers a blob miss for a column after a
world-change packet for that column has reached the queueing path, vanilla
leaves receive-side bucket 0 paused permanently. This is observed vanilla
behavior and is a remotely triggerable client hang.

## Chunk insertion is strictly sequence-ordered

`NetworkChunkInserter` (RVA `0x0799da90`) keeps a min-heap of
`{sequenceID, chunk}` entries and inserts chunks only while the heap's top
sequence ID equals `mNextChunkSequenceID`.

Blob completion order does not determine insertion order. Packet arrival
sequence does: a stuck earlier column blocks insertion of later columns.

## Block-update discard case

Vanilla silently discards a block update only when no chunk packet is pending
for the column. `handleUpdateBlock` (RVA `0x07991200`) resolves the target
through `hasBlock`, `getChunkAt`, `getChunk`, and then
`getAvailableChunk`/`getGeneratedChunk`. This lookup path never creates a
chunk, and a null chunk returns silently.

## Cache status under pressure

`LegacyClientNetworkHandler::_drainCacheMissesQueueAndSendPacket` (RVA
`0x07997dd0`) runs from `onTick` every `0x4c4b40` nanoseconds, or 5 ms. It
drains missing IDs first and found IDs second into one cache-status packet.

`isFull()` (RVA `0x09db9a00`) tests whether
`mMissingIds.size() + mFoundIds.size() >= 0xfff`. Missing IDs and found IDs
therefore share one 4,095-entry capacity. `isEmpty()` (RVA `0x09db9a30`) is
true only when both vectors are empty, and only a totally empty status packet
is suppressed.

A packet with an empty `mMissingIds` list and a populated `mFoundIds` list is
the normal vanilla shape for a chunk that is a full cache hit. A compliant
server must handle this packet.

At capacity, missing IDs can consume the entire 4,095-entry budget and defer
found-ID acknowledgements; leftovers remain queued for the next 5 ms drain.
Vanilla BDS accepts this. `ServerNetworkHandler::handle` (RVA `0x09c851e0`)
calls `dropBlobFor` for each missing ID and
`TransferTracker::onAckReceived` for each found ID, with no coupling between
the two lists.

## Corroborating public sources

These public sources corroborate parts of the binary observations but are not
the primary authority for this contract:

- Mojang's [blob-cache design note](https://gist.github.com/Tomcc/4be79d3eafcd158c5059abd4ab2e8d35)
  describes between one and eight concurrent transactions and a maximum of
  4,095 IDs in `ClientCacheBlobStatusPacket`.
- A public [cache-poisoning disclosure](https://gist.github.com/JustTalDevelops/1abfdae7ab7618af2ec82f709ffa93bb)
  reports that the vanilla client no longer validates a blob payload against
  its hash.

## Cinnabar divergences and open decisions

Cinnabar deliberately retains blob-payload hash validation even though
vanilla no longer performs it. This is a security-motivated divergence that
protects against cache poisoning.

Cinnabar does not currently replicate vanilla's permanent receive-side
bucket-0 pause as-is. Whether to reproduce vanilla's unbounded pause or
deliberately diverge with a bounded timeout remains open pending a decision
by the repository owner. This document does not resolve that choice.

Cinnabar currently resolves transactions out of order and uses per-column
ordering rather than vanilla's connection-wide receive pause. This is a known
divergence pending redesign, not a validated parity choice.

Implementing vanilla's pause requires no channel or transport concept: only
a receive-side pause with the two-bucket classification described above.
Cinnabar's existing cache/ordinary lane split approximates that mechanism;
aligning the pause condition with vanilla's per-column reference count remains
open work.
