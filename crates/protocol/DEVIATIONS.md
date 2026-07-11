# Protocol deviations

All entries below target Bedrock 1.26.30 / protocol 1001. Valentine/Jolyne is
based on axolotl-stack commit
`6f6806e821a579c183c44d786f76d9b358a2b825`; exact wire behaviour is checked
against gophertunnel commit
`9948b1729395d2e819fce28e079d4a7bfc67716c` and live BDS 1.26.32.2. Mojang
protocol documentation was not needed to distinguish these cases because the
pinned encoder/decoder and live bytes agreed exactly.

## Task 0.4 baseline

No drift appeared in the original five fixtures. `NetworkSettings`,
`StartGame`, `LevelChunk`, `MovePlayer`, and `AddActor` (Valentine `AddEntity`)
decoded to the expected fields and re-encoded byte-for-byte.

## Resolved findings from Task 0.5

### PlayerList has one shared entry count

Gophertunnel's `minecraft/protocol/packet/player_list.go` writes the action,
one entry count, exactly that many records, and--for Add only--exactly that many
trusted-skin booleans. Generated `PlayerRecords` inserted additional vector
counts before records and verification flags, desynchronising a live Add list.

The local generated patch keeps one `records_count`, rejects negative or more
than 4,096 decoded entries before allocation, and uses that count for both
conditional sections. Encoding rejects count/vector mismatches, action/record
mismatches, absent records, and inconsistent trusted-skin flags. The exact
Remove fixture in `tests/player_list.rs` is 21 bytes with SHA-256
`1c1432f38e374b2fa44263140d7b0aba1d64b685e4566a4bca5a541ad903c0bb`;
the owned path round-trips byte-for-byte and the borrowed path materialises the
same count and record type. A 90-byte one-entry Add fixture with SHA-256
`3beb026f11d8b29742c6e96e14b7fd1f9832bd5bf6b7da38715a5dfd7ae211dd`
separately proves that the trusted-skin boolean follows the record directly,
with no second count prefix, and re-encodes exactly.

### ItemLegacy uses both 0 and -1 as empty sentinels

Gophertunnel's item reader/writer returns immediately for network ID `0` or
`-1`; neither value carries count, metadata, block ID, or extra data. Generated
`ItemLegacy` only recognised `0`, so a creative group icon with `-1` consumed
the following packet fields as item content.

The patch applies the two-sentinel rule to every ItemLegacy use. The anonymous
creative-group fixture in `tests/creative_content.rs` is 12 bytes with SHA-256
`e6320ead1d619d075b7519f366815baec98b258a5b09bf3d217b8269bc365102`;
the owned path round-trips exactly and the borrowed path materialises the same
empty item.

### Item extra-data NBT is fixed-width little endian

Gophertunnel's item paths use `nbt.LittleEndian` for item user data, not
`nbt.NetworkLittleEndian`: strings use signed little-endian 16-bit lengths and
integers/array lengths use fixed little-endian widths. Valentine had one NBT
scanner using VarUInt strings and ZigZag integers. On the live enchanted-book
creative icon it consumed only `0a 00 00`, then misread `6e 63` as a 25,454-byte
string length.

`Nbt::decode_little_endian` now scans the fixed-width variant, and only the item
extra-data call site uses it. The exact enchanted-book fixture is 89 bytes with
SHA-256
`85e3f7083ae524ebe7f14b09da5f99d0e132cd2d5fe2d49d4cc23b06969a2d41`;
both owned fixtures re-encode exactly and both borrowed paths materialise the
expected fields.

### Shaped recipes have an implicit input length

Gophertunnel's `marshalShaped` reads/writes exactly `width * height`
`ItemDescriptorCount` values through `FuncSliceOfLen`; there is no outer input
count and no per-row count. Generated `RecipesItemRecipeShaped` used a nested
`Vec<Vec<...>>` and emitted both, desynchronising the live CraftingData packet.

The generated patch stores a flat input vector, validates its dimensions when
encoding, and derives the decode length from checked dimensions. The one-cell
fixture in `tests/crafting_data.rs` is 53 bytes with SHA-256
`33c86d633237c5da6fc59fc12de3e6cd5c01dddf9734326a20fe79f8ef6a2f73`;
the owned path round-trips exactly and the borrowed path materialises the same
one-cell recipe. After this patch the live login stream decoded CraftingData
and advanced to AvailableCommands.

## Task 0.5 boundary hardening

The encrypted bridge additionally carries the same bounded-input expectations
as the public protocol codec and pinned gophertunnel path:

- decoded batches are capped at 16 MiB for Deflate, Snappy, the `0xff`
  no-compression marker, and pre-NetworkSettings raw batches;
- raw batches are capped at 1,600 packet entries;
- owned and borrowed packet decoders reject bytes left inside the declared
  entry, with the borrowed path reporting exact consumption without an owned
  materialisation;
- network NBT and fixed little-endian item NBT both reject compound/list depth
  513 while accepting depth 512; and
- generated collections eagerly decoded for resource packs, StartGame, and
  ItemRegistry reject more than 4,096 elements and impossible counts before
  allocation (the same checks also cover optional definition packet schemas);
- the login state machine has one 120-second deadline, recognises the protocol
  `0xffff` no-compression negotiation, rejects non-empty pack stacks except
  gophertunnel's exact client-built-in exemptions, sends the radius request
  immediately after StartGame, and returns
  unrelated pre-spawn packets to PlaySession in wire order after compacting
  them under aggregate 1,600-packet/16 MiB limits.

Focused tests exercise every limit boundary, 16 malicious collection-count
cases, all three compression modes, both spawn/radius orders, delayed
ItemRegistry, compact FIFO deferred delivery and aggregate overflow, unsupported
pack stacks, Disconnect handling, and public send-header validation.

## Phase 0.8 protocol-1001 drift resolutions

### AvailableCommands (packet 76)

The Task 0.5 diagnostic observed a 356,513-byte live BDS body fail after
Valentine consumed the EnumValues count twice. The pinned gophertunnel commit
`9948b1729395d2e819fce28e079d4a7bfc67716c` uses one
`FuncSlice(EnumValues)` VarUInt count. Valentine had stored `values_len`, then
written and decoded a second vector prefix.

Task 0.8 removes the redundant field and derives exactly one count from
`enum_values` when encoding. Owned and borrowed/materialised decoding consume
exactly that many strings; decoding and encoding reject more than the pinned
gophertunnel 4,096-element slice limit, and allocation is fallible after count
and remaining-byte validation.

Pinned fixtures generated by that exact gophertunnel commit are:

- `available_commands.bin`: all eight packet sections, 165-byte raw batch,
  SHA-256 `3d6e1870c49d643fe3f3b901cbbba40f49768cfe408c8b6ee136b5304ac1c98f`;
- `available_commands_live_356513.bin`: the same complete shape expanded to the
  observed 356,513-byte live body length, 356,519-byte raw batch, SHA-256
  `08dea656b782928828fa79fc004166220f14d0459e259717bb537c5a11f6b39a`.

Tests cover owned decode, borrowed materialisation, exact byte re-encoding,
malformed and oversized shared counts, encoding bounds, and the recorded live
body-length regression. The guarded BDS login test now continues after
StartGame until packet 76 is decoded and then asserts zero decode errors.

### MaterialReducer in CraftingData (packet 52)

Pinned gophertunnel defines `Outputs []MaterialReducerOutput` and writes one
VarUInt count followed by every ZigZag `(network_id, count)` pair. Valentine
previously modelled and consumed one uncounted pair in both owned and borrowed
forms.

Task 0.8 replaces that singular field with a bounded output vector in both
forms, applies the same 4,096-element limit and fallible preallocation, and
encodes every output after one count. `material_reducer.bin` is an 18-byte
pinned-gophertunnel raw CraftingData batch with one reducer and two outputs,
SHA-256 `b73c651ccf07ece21aea4b186be3780875ce7cacef04f9327e3c968636d43a39`.
Owned decode, borrowed materialisation, the direct borrowed reducer view, exact
byte round-trip, oversized counts, and truncated vectors are covered.
