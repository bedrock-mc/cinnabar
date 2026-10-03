package replay

// Storage contract:
//
//   - One Store owns one directory (an exclusive advisory filesystem lock).
//   - All writers and reads share a mutex; codec concurrency is one worker.
//   - Call PutAsset for arena/skin bytes, then Begin or AddAsset to transfer
//     each reservation. ReleaseAsset releases an abandoned setup reservation.
//     Arena assets compress transparently; PNGs remain verbatim inside a small
//     versioned envelope. Individual assets are bounded to 64 MiB decoded.
//   - Append receives full opaque JSON snapshots and match-relative monotonic
//     milliseconds. It must run outside a game world transaction.
//   - Chunks span at most one second or 4 MiB of uncompressed snapshot lines.
//     Pressure may create shorter chunks. A seek starts independently at any
//     indexed chunk; smooth interpolation is a playback responsibility.
//   - Pending frame data across all matches is capped at 16 MiB, active matches
//     at 64, and each index at 8192 chunks. Metadata detail is at most 256 KiB.
//   - Finish flushes, fsyncs and atomically publishes. Aborted or interrupted
//     recordings never appear in the completed list.
//   - The quota counts logical file bytes for chunks, indexes and deduplicated
//     assets, including active data and temporary writes. Filesystem block and
//     inode overhead are outside this application byte quota.
//   - Oldest completed recordings are removed first. Shared assets survive
//     while referenced by any completed/active recording or setup reservation.
//   - Restart discards unfinished recordings and unreferenced assets, validates
//     completed indexes/asset hashes, then enforces the configured byte quota.
//
// The format stores only the bytes supplied by capture. It cannot invent
// missing equipment, projectiles, sounds or physics events. A full-state
// payload avoids delta dependencies at seeks; zstd compresses repeated fields
// within each time chunk. Chunk format and manifest are explicitly versioned.
