# Block-entity static/logical evidence design

## Scope

This tranche closes only the block entities whose visible result does not need a
new block-entity geometry pipeline:

- `Barrel`, `BlastFurnace`, `Furnace`, and `Smoker` keep using their existing
  block-state cube visuals. Their inventory, name, burn, cook, and XP NBT does
  not select a second render object.
- `Jukebox` and the id-less `Note` producer add no block-entity geometry. Their
  backing blocks must remain GPU-presented while NBT changes produce zero extra
  block-entity references.

This does not claim `Bed`, `BrewingStand`, or `Hopper`. Those 36 canonical states
are still diagnostic. Bed also requires its NBT `color` to select visible
materials, so it belongs to a later geometry-and-NBT tranche. Animated,
custom-geometry, and text-overlay entries remain deferred.

The reviewed result is six proven renderers and sixteen deferred renderers. The
global strict-final gate remains red until all 22 are proven.

## Rejected shortcuts

1. Do not mark manifest entries implemented by inserting plausible witness
   strings. The current generator accepts arbitrary non-empty identifiers; that
   is bookkeeping, not evidence.
2. Do not classify the four existing cube visuals as invisible. The block is
   visible; only the additional block-entity draw contribution is empty.
3. Do not infer Note solely from an absent `id`. Identification requires the
   exact noteblock backing state plus bounded root `note` and `powered` fields.
4. Do not add a per-block-entity Bevy `Mesh`, material, bind group, or GPU
   buffer. Existing packed chunk streams remain authoritative.
5. Do not treat GPU routing evidence as vanilla geometry/UV parity evidence for
   later Bed, BrewingStand, or Hopper work.

## Evidence catalog

Add a deterministic, version-pinned block-entity evidence catalog beside the
existing inventory and renderer manifest. Each evidence record binds:

- protocol/game version;
- BREG, MCBEAS, block-entity source-contract, renderer-contract, and
  gallery-request SHA-256 identities. The two contract hashes are canonical
  projections that exclude renderer status and all evidence/witness fields, so
  promoting validated evidence cannot create a circular hash dependency;
- source key, required variant ID, NBT SHA-256, absolute block position,
  canonical state, sequential ID, and network hash;
- expected backing stream and backing reference count;
- expected additional block-entity reference count;
- two adjacent GPU-completed frame receipts with exact generation, stream,
  reference count, and digest values.

The generator joins manifest witness IDs to catalog records. It rejects unknown
witnesses, duplicate ownership, wrong source/variant/kind, stale artifact
hashes, incomplete variant domains, wrong backing identities, non-adjacent
frames, and contradictory GPU/no-additional-draw claims. Reviewed mode permits a
partial catalog and reports exact proven/deferred counts. Strict-final mode
still requires all 22 entries.

Catalog parsing is bounded and canonical. Evidence files contain hashes and
measurements only; no Mojang textures, BDS payloads, or screenshots are tracked.

## Runtime adjudication

Extend the bounded NetworkLittleEndian NBT root scan only with the typed scalar
fields needed for this tranche: tag-1 byte `note` in `0..=24` and tag-1 byte
`powered` restricted to `0` or `1`. Duplicate fields, wrong tag types, and
out-of-range values fail closed. Preserve exact source bytes.

Introduce one pure block-entity visual classifier with explicit outcomes:

- `ExistingBlockState`: the four static cube sources;
- `LogicalNoAdditionalDraw`: Jukebox and a correctly discriminated id-less
  Note;
- `Deferred`: every other reviewed source;
- `Unknown`: mismatched source/backing combinations.

The classifier receives the block entity, backing canonical state identity, and
compiled visual facts. It never changes the backing block's normal cube/model
route. It produces a stable evidence identity and an exact expected additional
reference count of zero for this tranche.

NBT changes for these six entries must not dirty or remesh the owning subchunk.
The backing block packet remains the only way to change their visible block
state. Session replacement, dimension replacement, block-entity eviction, and
malformed updates preserve the existing fail-closed sparse-store behavior.

## Deterministic gallery and GPU receipts

Add a block-entity gallery request/response path that reuses the acceptance
harness's fixture layout, committed-camera fence, packed chunk renderer,
`PresentedFrameAck`, and direct/MDI accounting. Do not reuse the current model
witness format unchanged because it binds only subchunk keys and nonzero model
references.

Every target binds one absolute position and one exact palette/NBT identity. Put
targets in isolated subchunks where subchunk-level counters could otherwise
alias. Cover every declared required NBT variant, plus the full Note domain of
pitch `0..24` by powered `false/true`. For the four cube sources and Jukebox,
prove the backing cube is present in two adjacent GPU-completed frames while the
additional block-entity reference count stays zero. For Note, prove the same
result for every pitch/powered combination and reject any absent-id record that
does not have the exact noteblock backing identity and typed fields.

The acceptance producer writes a local candidate catalog atomically. The
generator validates it before tracked artifacts or manifest claims change.

## Tests and gates

- World: exact typed-field decoding, duplicates, wrong types/ranges, id-less
  Note discrimination, malformed retention, and byte/depth/collection bounds.
- App: inline, request-mode, and live-update parity; zero remesh for all six;
  session/dimension reset; eviction; malformed update retention.
- Assets/render: exact existing backing visual identity; zero diagnostic
  material for the four cube sources and Jukebox/Note backing blocks; direct/MDI
  parity; no additional block-entity draw allocations.
- Evidence generator: every catalog join failure mode above, deterministic
  output, six-proven/sixteen-deferred reviewed result, and strict-final still
  red.
- Acceptance: wrong state/NBT/hash/stream/ref count, stale or missing frame,
  non-adjacent receipt, cross-target contamination, and clean two-frame success.
- Repository: full Rust workspace tests, strict all-target Clippy, formatting,
  every Go module in its intended workspace mode, diff hygiene, no tracked
  Mojang payload, independent review, then push to `phase2-textures`.

## Follow-up order

After this tranche, implement Bed, BrewingStand, and Hopper with legitimate
version-matched geometry/UV authority and sparse Bed-color mesh invalidation.
Then proceed to animated/custom/text block entities. No later tranche may weaken
the evidence-catalog joins established here.
