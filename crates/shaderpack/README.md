# Shader-pack loading

`shaderpack` loads Java Edition OptiFine/Iris shader packs from a folder (the pack or its
`shaders/` directory), a ZIP archive (`shaders/` at the root or under one top-level folder)
or in-memory text. `PackSource::files` lists text in path order; `PackSource::read_bytes`
reads any asset, including binaries, without leaving `shaders/`. Ingestion is bounded per
file, per pack and by entry count.

Configuration parsing, shader translation and render-host integration are not implemented
yet; loading a pack does not activate it.

```sh
cargo test --locked -p shaderpack
```
