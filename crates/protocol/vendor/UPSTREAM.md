# Pinned upstream sources

The protocol crate resolves Valentine and Jolyne from the checked-in
`vendor/valentine` and `vendor/jolyne` paths. The retained source is MIT
licensed; see `crates/protocol/vendor/LICENSE`.

Machine-checked provenance:

- Dependency resolution: local vendored paths
- Reviewed fork revision: `6cd8087fc3f0b500e41708a8afc94a0fa3291525`
- Upstream snapshot revision: `6f6806e821a579c183c44d786f76d9b358a2b825`
- Generated 1.26.40 source revision: `781dfcb0ab443476b62df3c983750c0c1527a95a`
- Retained license normalized SHA-256: `62c75fcb256604584191434b605dc3fe661d938a94b2c35836ef55011bf24184`

The copied surface contains the shared codec/runtime, the generated protocol
2168 crate, and the Jolyne client/server transport facade. Upstream examples,
benches, generator executables, unrelated workspace crates, and uninitialised
generator-input submodules are omitted. Local manifests replace workspace
inheritance with direct versions and local paths.

## Local source patches

The retained Jolyne changes preserve negotiated compression, bounded batch
ingress, deferred packets, strict login sequencing, compact raw-frame error
context, and exact packet-entry boundary checks. The shared codec includes a
fixed-width little-endian NBT scanner with bounded nesting and Bedrock UUID
encoding as two little-endian `u64` halves.

The generated protocol crate includes reviewed wire corrections for binary
buffers, little-endian scalar union arms, strict actor-ID varints, adjacent
redactable strings, and opaque preservation of unavailable packet bodies.
Pinned conformance fixtures under `crates/protocol/tests` cover these shapes.

Wire behaviour and byte fixtures use the project pin
`hashimthearab/gophertunnel` commit
`56a0f77dbbb2fb006b081ec38bb4bedf9cb95088` (`cinnabar`, module pseudo-version
`v1.25.3-0.20260807205305-56a0f77dbbb2`, Minecraft 1.26.40 / protocol 2168).

## Generated-code caveats

The generated 1.26.40 decoders do not add collection-allocation guards before
every eager vector allocation. Cinnabar's pre-decode gates bound the exposed
login, inventory, and text surfaces; dedicated tests pin the current EOF
behaviour where generator-level bounds remain future work.

`LevelChunkPacketView::serialized_chunk_data` remains owned rather than
zero-copy. This is an acknowledged generator follow-up and does not change its
wire framing.

Content registries are maintained independently of the generated wire schema.
The protocol crate uses reviewed retail item and biome allowlists under
`crates/protocol/data/`.
