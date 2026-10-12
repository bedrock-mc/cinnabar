# Pinned upstream sources

`bedrock-protocol-1-26-51/` is protocolgen's generated Rust contract crate for Bedrock 1.26.51
(protocol 2193), copied byte for byte with no local patches. Only `protocol` depends on it.

- Generator: https://github.com/bedrock-mc/protocolgen
- Generator commit: `dec7334ac3981b0b79146fa53c82af91d70f9f62` (`generated/1.26.51/rust-contract`)

The expected protocol, codec contract and manifest fingerprint are pinned once, in
`assets/bedrock-target.json`; a protocol test fails until a re-vendored crate matches them. Protocol 2193 is spoken by Minecraft
1.26.50 through 1.26.52, and the client reports game version 1.26.50 to match gophertunnel.

The raw packet entries and spawn sequence (`src/raw_packet.rs`, `src/login/startup.rs`) are
adapted from Jolyne, the Axolotl Stack client session library.

- Axolotl Stack merge revision: `c4540512dc47833bb40363da7ad1161110d64b67`
- Retained license normalized SHA-256: `62c75fcb256604584191434b605dc3fe661d938a94b2c35836ef55011bf24184`

Jolyne is MIT licensed, Copyright (c) 2025 Jeremy Ianne; its license is `LICENSE` here, also
reproduced in `THIRD_PARTY_NOTICES.md`.

Content registries are maintained independently of the generated wire schema. The protocol crate
uses reviewed retail item and biome allowlists under `crates/protocol/data/`.
