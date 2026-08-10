# Blob-cache safety and public reference notes

## Public references

- Mojang's public [blob-cache design note](https://gist.github.com/Tomcc/4be79d3eafcd158c5059abd4ab2e8d35)
  documents the cache-status exchange and its 4,095-ID packet bound.
- A public [cache-poisoning disclosure](https://gist.github.com/JustTalDevelops/1abfdae7ab7618af2ec82f709ffa93bb)
  motivates retaining payload-hash validation as a security boundary.

These sources do not establish a complete version-matched client behavior
contract. Native parity for ordering, pause behavior, persistence, trimming,
and concurrency remains open until it has reproducible public evidence.

## Cinnabar implementation boundaries

Cinnabar rejects cached LevelChunk hash lists above 4,096 during bounded wire
decode. It validates every received blob against its advertised hash and treats
a mismatch as cache poisoning rather than storing the payload.

The resolver applies explicit limits to retained transactions, per-transaction
reconstruction, staged payload, aggregate ready payload, and aggregate pending
bytes. Pressure abandons the affected transaction non-fatally through the
existing recovery path while preserving already retained work. The exact limits
and recovery behavior are implemented and tested alongside the resolver; they
are safety choices, not claimed vanilla limits.

The current cache is byte-bounded and in-memory. Persistent storage and a
version-matched trim schedule remain open parity work. Current ordering and
backpressure behavior likewise remain provisional until the public native gate
is complete. Implementation defects are tracked separately in the
[blob-cache open-defects ledger](blob-cache-open-defects.md).
