# Open Font Review Repairs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Repair the four independent review findings in commit `1b9006a` while preserving automatic Inter acquisition and explicit local bitmap-font support.

**Architecture:** The asset compiler will validate the outline source against a typed tracked manifest before rasterization and will hash canonical LF manifest bytes. Make will keep generated Inter and explicit local bitmap fonts in separate carriers, and startup will prefer a validated local carrier when present. Repository metadata will force portable LF checkout for the manifest and retain the exact upstream OFL bytes.

**Tech Stack:** Rust, Cargo integration tests, GNU Make, PowerShell/Bash acquisition scripts, Git attributes.

## Global Constraints

- Use red-green-refactor TDD for every behavioral change.
- Keep rasterization and all file reads bounded.
- Do not fetch or redistribute Mojangles.
- Do not push commits.

---

### Task 1: Enforce the pinned outline source identity

**Files:**
- Modify: `crates/asset-compiler/tests/public_outline_font_api.rs`
- Modify: `crates/asset-compiler/src/bin/assetc.rs`

**Interfaces:**
- Consumes: `font_size_bytes` and `font_sha256` from `assets/ui-font-source.json`.
- Produces: CLI rejection before rasterization when either source property differs.

- [x] Add an integration test invoking `assetc outline-font-assets` with bytes that do not match the manifest pin and assert no output or report is published.
- [x] Run the test and confirm it fails because the current CLI reaches outline parsing instead of reporting a source identity mismatch.
- [x] Deserialize and validate the manifest source size/hash before calling `compile_outline_font`.
- [x] Run the focused test and confirm it passes.

### Task 2: Make manifest provenance portable across line endings

**Files:**
- Modify: `crates/asset-compiler/tests/public_outline_font_api.rs`
- Modify: `crates/asset-compiler/src/bin/assetc.rs`
- Modify: `.gitattributes`

**Interfaces:**
- Consumes: LF or CRLF JSON manifest bytes.
- Produces: the same source-manifest identity used by startup for either checkout representation.

- [x] Add a test proving CRLF and LF outline manifests produce the canonical tracked hash.
- [x] Run it and confirm raw-byte hashing fails the parity assertion.
- [x] Canonicalize all-CRLF manifests to LF before hashing and force `assets/ui-font-source.json text eol=lf`.
- [x] Run the focused compiler and startup parity tests.

### Task 3: Preserve explicit local bitmap-font selection

**Files:**
- Modify: `app/tests/assets.rs`
- Modify: `app/src/app.rs`
- Modify: `app/src/asset_startup.rs`
- Modify: `Makefile`
- Modify: `README.md`

**Interfaces:**
- Consumes: `make font-assets-local FONT_PACK_DIR=...`.
- Produces: a separate `vanilla-v1.mcbefont` local carrier which startup prefers over the generated `ui-inter-v1.mcbefont` carrier.

- [x] Add Make/startup tests for distinct carrier paths and validated local-carrier precedence.
- [x] Run them and confirm both commands currently overwrite the same path and startup ignores the local path.
- [x] Add separate Make outputs, startup selection, and documentation.
- [x] Run the focused Make contract tests.
- [x] Add a diagnostic-startup summary regression and make the application log the selected font state honestly.

### Task 4: Restore exact license bytes and verify all behavior

**Files:**
- Modify: `assets/licenses/Inter-OFL-1.1.txt`
- Modify: `crates/asset-compiler/tests/public_outline_font_api.rs`

**Interfaces:**
- Consumes: pinned upstream OFL content/hash.
- Produces: a tracked license file with exact SHA-256 `5b9321a4298cfeb6b34354164a1c3afc3db114569984c502b9b35d988fd58c57`.

- [x] Add a hash assertion for the tracked license and confirm it fails.
- [x] Restore the exact upstream line wrapping and confirm the hash test passes.
- [x] Run actual tampered-font rejection, CRLF parity, local selection behavior, full focused tests, strict Clippy, formatting, and diff checks.
- [ ] Commit the reviewed repair set without pushing.
