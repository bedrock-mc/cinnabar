# Vendored upstream sources

Task 0.4 vendors protocol definitions and session code from
[`axolotl-stack/axolotl-stack`](https://github.com/axolotl-stack/axolotl-stack)
merge commit `6f6806e821a579c183c44d786f76d9b358a2b825` under the upstream MIT license.

Copied paths:

- `crates/valentine/src`
- `crates/valentine/bedrock_core`
- `crates/valentine/bedrock_versions/v1_26_0`
- `crates/valentine/bedrock_versions/v1_26_30`
- `crates/valentine/Cargo.toml` and `README.md`
- `crates/jolyne/src`
- `crates/jolyne/Cargo.toml` and `README.md`
- root `LICENSE`

Upstream examples, tests, benches, generator executable, unrelated workspace
crates, and uninitialised generator-input submodules are omitted. Local Cargo
manifests replace workspace inheritance with direct versions and local paths;
upstream-only development dependencies and targets are removed. Jolyne doctests
are disabled because its copied README demonstrates the omitted RakNet transport.
Jolyne defaults to no features, retains its feature names for cfg checking, and
has only three source changes: cfg guards around its RakNet-only import,
transport import, and client connection impl. No generated Valentine source is
modified.

The generated Valentine data records these upstream generator inputs:

- PrismarineJS `minecraft-data` commit
  `6ec59288287e4045331eaa47ee8fb104278f6b98` (MIT)
- pmmp `BedrockData` commit
  `7d74ffbdd620dc1e31af0a645d3eea738c820c0b` (CC0-1.0)

The byte fixtures are generated with the project's pinned gophertunnel commit
`9948b1729395d2e819fce28e079d4a7bfc67716c`.
