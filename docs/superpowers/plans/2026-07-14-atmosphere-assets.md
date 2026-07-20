# Phase 2.7 Atmosphere Assets Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add deterministic, bounded compilation and runtime carriage for the pinned vanilla sun, moon-phase, and cloud textures.

**Architecture:** A focused `atmosphere` module owns the fixed source contract and independent `MCBEATM1` encoder/decoder. The existing `assetc` binary adds one subcommand that writes the blob and deterministic provenance report; the world `MCBEAS05` format and render crate remain unchanged.

**Tech Stack:** Rust 2024, `image`, `sha2`, `serde`/`serde_json`, `clap`, Cargo integration tests.

## Global Constraints

- Use only the pinned Mojang `bedrock-samples` resource pack described by `assets/vanilla-source.json`.
- Keep Mojang files ignored/untracked and never copy them into the worktree.
- Do not edit `crates/render`, shaders, plugins, app loading, or GPU code.
- Fail closed for absent, malformed, oversized, unsafe, or dimensionally incorrect assets.
- Output and reports must be byte-deterministic and carry exact paths and hash provenance.

---

### Task 1: Fixed atmosphere compiler contract

**Files:**
- Create: `crates/assets/tests/atmosphere.rs`
- Create: `crates/assets/src/atmosphere.rs`
- Modify: `crates/assets/src/lib.rs`
- Modify: `crates/assets/src/error.rs`

**Interfaces:**
- Produces: `compile_atmosphere_assets(root: &Path, source_manifest: &[u8]) -> Result<CompiledAtmosphereAssets, AssetError>` and immutable `AtmosphereTexture` records.

- [ ] Write synthetic-pack tests asserting fixed order, paths, dimensions, file/pixel hashes, manifest hash, and missing/malformed/oversized/wrong-dimension rejection.
- [ ] Run `cargo test -p assets --test atmosphere compiler_ --locked -- --nocapture` and confirm failures are caused by the absent API.
- [ ] Implement only the fixed three-source reader, bounded PNG decoder, metadata types, and errors required by those tests.
- [ ] Re-run the focused compiler tests and confirm they pass.

### Task 2: Versioned binary and runtime decoder

**Files:**
- Modify: `crates/assets/tests/atmosphere.rs`
- Modify: `crates/assets/src/atmosphere.rs`
- Modify: `crates/assets/src/lib.rs`

**Interfaces:**
- Consumes: `CompiledAtmosphereAssets` from Task 1.
- Produces: `encode_atmosphere_blob(&CompiledAtmosphereAssets) -> Result<Box<[u8]>, AssetError>` and `RuntimeAtmosphereAssets::decode(&[u8]) -> Result<Self, AssetError>`.

- [ ] Add tests for byte-identical repeated encoding, exact metadata/pixel round trip, canonical layout, and rejection of damaged magic/version/count/offset/path/dimension/source hash/pixel hash/envelope hash.
- [ ] Run `cargo test -p assets --test atmosphere blob_ --locked -- --nocapture` and observe expected missing-API failures.
- [ ] Implement the bounded `MCBEATM1` header, fixed descriptors, path/payload sections, trailing SHA-256, and allocation-safe decoder.
- [ ] Re-run the focused blob/runtime tests and confirm they pass.

### Task 3: CLI and deterministic provenance report

**Files:**
- Modify: `crates/assets/tests/atmosphere.rs`
- Modify: `crates/assets/src/bin/assetc.rs`
- Modify: `Makefile`
- Modify: `app/tests/assets.rs`

**Interfaces:**
- Consumes: Task 1 compiler and Task 2 encoder.
- Produces: `assetc atmosphere --pack ... --source-manifest ... --out ... --report ...`.

- [ ] Add command-level tests for documented help, deterministic binary/report output, full manifest provenance, exact texture metadata, no machine-local path in the report, failure-safe bundle publication, and Make dependency wiring.
- [ ] Run the command-focused tests and observe failures because the subcommand is absent.
- [ ] Implement the subcommand using bounded manifest reads, canonical JSON serialization, preflighted per-file atomic writes, the atmosphere compiler/encoder, and portable Make dependency wiring.
- [ ] Re-run command-focused and complete atmosphere tests.

### Task 4: Pinned-source ratchet and closure

**Files:**
- Modify: `crates/assets/tests/atmosphere.rs`
- Create: `.superpowers/sdd/phase27-atmosphere-assets-report.md`

**Interfaces:**
- Consumes: `PINNED_VANILLA_PACK` and the tracked manifest.
- Produces: exact pinned-path/hash evidence and handoff notes for renderer ownership.

- [ ] Add an environment-gated test for the pinned sun, moon, and cloud paths, dimensions, and source hashes.
- [ ] Run the pinned test against the legitimate local pack and confirm it passes.
- [ ] Run `cargo test -p assets --locked`, `cargo clippy -p assets --all-targets --all-features --locked -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check`.
- [ ] Audit `git status` and tracked file extensions to prove no Mojang payload is tracked, write the report, repeat verification, and commit locally without pushing.
