# Celestial Compositing Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace RGB-keyed replacement composition with decoded-asset-tested emissive sun/moon composition and log enough identity data to reject stale live evidence.

**Architecture:** Keep the existing fullscreen atmosphere pass and texture bindings. Add small pure Rust evidence helpers beside the atmosphere asset runtime types, mirror the same additive equation in one WGSL helper, and expose asset plus shader identities through the existing startup reporting path.

**Tech Stack:** Rust, Bevy/wgpu/WGSL, `MCBEATM1`, Naga, SHA-256, Cargo tests.

## Global Constraints

- Do not commit Mojang assets, generated `MCBEATM1` blobs, screenshots, or native renderer binaries.
- Preserve the existing sun/moon mapping, phase order, horizon visibility, texture-array identity, and atmosphere pipeline ABI.
- A celestial source texel must never darken any destination sky channel.
- Do not use RGB thresholding as opacity; black and near-black are emissive values, not replacement coverage.
- Write and run each regression test red before changing production behavior.

---

### Task 1: Decoded celestial evidence and additive composition

**Files:**
- Modify: `crates/assets/src/atmosphere.rs`
- Modify: `crates/assets/src/lib.rs`
- Modify: `crates/assets/tests/atmosphere.rs`
- Modify: `crates/render/src/atmosphere.wgsl`
- Modify: `crates/render/tests/atmosphere.rs`

**Interfaces:**
- Produces a pure RGB composition helper or equivalent testable value type whose equation is `destination + source * coverage` without early clamping.
- Produces a deterministic iterator/helper over the outer border of the 32×32 sun and each 32×32 moon phase tile from a decoded `RuntimeAtmosphereAssets` value.
- WGSL produces `composite_celestial(destination, sampled_rgb, coverage)` and both sun and moon use it.

- [ ] **Step 1: Write failing decoded-border and composition tests**

Build a deterministic `MCBEATM1` fixture whose sun border contains `[1, 1, 0, 255]` and whose moon tile borders contain `[0, 0, 1, 255]`. Decode it through `RuntimeAtmosphereAssets::decode`, enumerate every expected border coordinate once, and assert additive composition over `[0.02, 0.03, 0.04]` and `[0.8, 0.7, 0.6]` never decreases a channel. Assert a dark lunar interior texel still adds its nonzero channels.

- [ ] **Step 2: Run the focused tests and record the expected failures**

Run `cargo test -p assets --test atmosphere celestial --locked` and `cargo test -p render --test atmosphere celestial --locked`. Expected: failures because decoded border evidence and additive shader composition do not exist and the current shader replacement-mixes RGB-keyed samples.

- [ ] **Step 3: Implement the smallest decoded-border and composition interfaces**

Keep the helpers independent of filesystem paths and source PNG decoding: they consume only validated runtime atmosphere textures. Reject an unexpected sun or moon dimension instead of silently changing the tile contract. Use floating-point source values in `[0,1]` and preserve HDR results above one.

- [ ] **Step 4: Replace the shader behavior with one additive helper**

Use this behavior in WGSL:

```wgsl
fn composite_celestial(
    destination: vec3<f32>,
    sampled_rgb: vec3<f32>,
    coverage: f32,
) -> vec3<f32> {
    return destination + sampled_rgb * coverage;
}
```

`sample_sun` and `sample_moon` return sampled RGB plus geometric/horizon coverage only. The fragment entry point calls the helper for each body. Remove `celestial_opacity` and both replacement `mix` calls.

- [ ] **Step 5: Run focused and crate-level verification**

Run `cargo test -p assets --test atmosphere --locked`, `cargo test -p render --test atmosphere --locked`, and `cargo test -p render --locked`. Expected: all pass with Naga parsing and validating the changed shader.

- [ ] **Step 6: Commit**

Commit the design, plan, tests, and production behavior together with `fix: composite celestial textures additively`.

### Task 2: Live-evidence identities

**Files:**
- Modify: `app/src/asset_startup.rs`
- Modify: `app/src/main.rs`
- Modify: `app/tests/assets.rs`
- Test: the closest existing startup/log contract tests in `app/tests`

**Interfaces:**
- Startup reporting exposes the full lowercase-hex `MCBEATM1` envelope identity already computed at load time.
- Startup reporting exposes a stable SHA-256 identity of the compiled `crates/render/src/atmosphere.wgsl` source.
- Neither identity includes a local path, credential, Mojang byte payload, or machine-specific value.

- [ ] **Step 1: Write failing startup identity tests**

Assert two different synthetic atmosphere blobs produce different reported envelope hashes, an unchanged blob is stable across loads, and the shader identity equals SHA-256 of the exact `include_bytes!` source used to build the app.

- [ ] **Step 2: Run the focused app tests and record the expected failure**

Run `cargo test -p bedrock-client --test assets atmosphere --locked`. Expected: failure because the startup output currently reports only the atmosphere path and not both identities.

- [ ] **Step 3: Implement deterministic identity reporting**

Thread the existing atmosphere identity through the startup summary and compute the shader source identity at compile time/runtime from `include_bytes!` without reading the source tree. Emit one ordered, secret-safe log line.

- [ ] **Step 4: Run full verification**

Run `cargo test -p bedrock-client --test assets --locked`, `cargo test -p render --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo fmt --all -- --check`. Expected: zero failures, errors, warnings, or formatting diffs.

- [ ] **Step 5: Commit**

Commit with `feat: fingerprint atmosphere live evidence`.

