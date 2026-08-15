# Pinned upstream sources

The protocol crate resolves Valentine and Jolyne from the checked-in
`vendor/valentine` and `vendor/jolyne` paths. The retained source is MIT
licensed; see `crates/protocol/vendor/LICENSE`.

Machine-checked provenance:

- Dependency resolution: local vendored paths
- Axolotl Stack merge revision: `4b71f85bd9a07fc7e4ab36a6c2417bf44403675b`
- Protocolgen submodule, manifest, and generated-source revision: `c360b03369dd69059c85c9aef018e04b1cfa2c37`
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

The generated protocol crate is lowered from protocolgen's strict canonical
1.26.40 manifest. Reconciliation requires two byte-equivalent complete source
claims or a fingerprinted adjudication with independent wire evidence. Reviewed
corrections cover binary buffers, little-endian scalar union arms, strict
actor-ID varints, adjacent redactable strings, the two-selector PlayerList
entry layout, and opaque preservation of unavailable packet bodies. Pinned conformance fixtures under
`crates/protocol/tests` cover these shapes.

Protocolgen's independent Gophertunnel oracle was evaluated at
`hashimthearab/gophertunnel` commit
`be6713da4dc051a4197f897d04835e89e9c54321`. The runtime Go module pin below is
maintained separately and remains the authority for Cinnabar's server tooling.

Wire behaviour and byte fixtures use the project pin
`hashimthearab/gophertunnel` commit
`9f42f3679a573fc4b51104569cc4f422036e28ec` (`cinnabar`, module pseudo-version
`v1.25.3-0.20260811002754-9f42f3679a57`, Minecraft 1.26.40 / protocol 2168).

## Generated-code caveats

The generated 1.26.40 decoders validate signed and platform-sized lengths and
grow decoded collections through fallible allocation without trusting untrusted
wire counts for eager capacity. Byte buffers validate their declared size against
the remaining packet before allocating. These checks do not impose a global
collection ceiling; valid larger values remain accepted where the field contract
permits them.

Content registries are maintained independently of the generated wire schema.
The protocol crate uses reviewed retail item and biome allowlists under
`crates/protocol/data/`.
