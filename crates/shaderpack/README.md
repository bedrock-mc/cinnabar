# Shader-pack loading

`shaderpack` loads Java Edition OptiFine/Iris shader packs from a folder, ZIP
archive or in-memory text files. Open a pack with `PackSource::open`, inspect its
`PackSource::origin`, enumerate text in sorted path order with `PackSource::files`
and read text by its `shaders/`-relative name with `PackSource::text`.

Folders may contain `shaders/` or be that directory themselves. ZIP archives may
hold `shaders/` at the root or under one top-level folder. Directory roots are
canonicalized. Known binary extensions and files with NUL bytes in their first
8 KiB are excluded from the text view. Text has its UTF-8 BOM removed and invalid
UTF-8 replaced. `PackSource::from_files` accepts caller-owned text without the
filesystem ingestion limits.

`PackSource::read_bytes` reads binary assets as well as text. It rejects absolute
paths and parent traversal. Directory targets must resolve inside the shaders
root; concurrent filesystem changes between resolution and opening are outside
that check's guarantee. Directory symlinks are skipped during text ingestion.
Binary reads are bounded separately from the eagerly retained text view.

Filesystem ingestion bounds file bytes, total inflated and decoded text, entry
count and path bytes. ZIP ingestion also preflights its index and bounds actual
compressed reads and read/seek work, including repeated payload reads. ZIP64,
multiple disks, ambiguous end records and prepended/trailing payloads are outside
the supported container policy. Limits are private loading policy.

Configuration parsing, shader translation and render-host integration remain
unimplemented. Loading a pack does not activate it in the client or establish
that it renders correctly. The crate has no Bevy, wgpu or shader compiler dependency.

Small source and public API tests generate their own inputs without game assets:

```sh
cargo test --locked -p shaderpack --lib source::tests
cargo test --locked -p shaderpack --test loading
```
