# Phase 2.7 Celestial Compositing Report

Date: 2026-07-15

Base commit: `36ddd9820851c19f63082891e29b6080f8e4f967`

Branch: `phase27-atmosphere-parity-fix`

## Outcome

Tasks 1 and 2 of the approved celestial-compositing plan are implemented in
two local commits. The fullscreen atmosphere pass now composes the pinned sun
and moon as emissive RGB using `destination + sampled_rgb * coverage`, without
RGB-keyed opacity, replacement mixing, or early HDR clamping. Startup emits one
ordered evidence line containing the full lowercase-hex `MCBEATM1` envelope
SHA-256 and the SHA-256 of the WGSL source embedded in the app.

No cloud code, `plan.md`, Mojang payload, generated atmosphere blob, screenshot,
or other worktree was changed. Nothing was pushed.

Status: `DONE`. All implementation and automated verification gates are green.
The independent review subsequently returned one Important finding about a
machine-specific path in the identity evidence line; that finding is resolved
and verified in the follow-up recorded below. Native GDI visual acceptance
remains the later integration gate described by the approved design.

## Commits

- `0166ea6a6479e3a15745f8b3c5f2855d23a00b0d` --
  `fix: composite celestial textures additively`
- `a8b0dfbddc7895770ce2cd2c8eed3946e998ad10` --
  `feat: fingerprint atmosphere live evidence`

## Changed files

Task 1:

- `docs/superpowers/specs/2026-07-15-celestial-compositing-parity-design.md`
- `docs/superpowers/plans/2026-07-15-celestial-compositing-parity.md`
- `crates/assets/src/atmosphere.rs`
- `crates/assets/src/lib.rs`
- `crates/assets/tests/atmosphere.rs`
- `crates/render/src/atmosphere.wgsl`
- `crates/render/tests/atmosphere.rs`

Task 2:

- `app/src/asset_startup.rs`
- `app/src/main.rs`
- `app/tests/assets.rs`

This report is the only additional artifact.

## TDD evidence

Baseline before production edits:

- `cargo test -p assets --test atmosphere --locked` -- 15 passed.
- `cargo test -p render --test atmosphere --locked` -- 16 passed.
- `cargo test -p bedrock-client --test assets --locked` -- 35 passed.

Task 1 RED:

- `cargo test -p assets --test atmosphere celestial --locked` failed to
  compile on the deliberately requested missing `CelestialTile`,
  `composite_celestial`, and `celestial_border_texels` interfaces.
- `cargo test -p render --test atmosphere celestial --locked` ran two tests
  and failed the new additive-composition assertion because
  `composite_celestial` was absent from WGSL.

Task 1 GREEN:

- `cargo test -p assets --test atmosphere celestial --locked` -- 2 passed.
- `cargo test -p render --test atmosphere celestial --locked` -- 2 passed.
- `cargo test -p assets --test atmosphere --locked` -- 17 passed.
- `cargo test -p render --test atmosphere --locked` -- 16 passed, including
  Naga parse and validation of the changed WGSL.
- `cargo test -p render --locked` -- exit 0 for the complete render package.

The decoded evidence fixture is encoded as a deterministic `MCBEATM1` envelope
and decoded through `RuntimeAtmosphereAssets::decode`. It visits 124 unique
outer-border coordinates for the sun and for each of eight moon tiles. The sun
border is `[1, 1, 0, 255]`; every moon border is `[0, 0, 1, 255]`. Both bright
and dark destination skies are checked for channel-wise non-darkening. A dark
lunar source retains its nonzero contribution, and an HDR result above one is
proved not to clamp early.

Task 2 RED:

- `cargo test -p bedrock-client --test assets atmosphere --locked` failed to
  compile on the deliberately requested missing shader-source hash, evidence,
  and startup-summary APIs.

Task 2 GREEN:

- `cargo test -p bedrock-client --test assets atmosphere --locked` -- 7 passed.
- `cargo test -p bedrock-client --test assets --locked` -- 37 passed.
- `cargo test -p render --locked` -- exit 0 for the complete render package.

The app test loads two different synthetic atmosphere envelopes and proves
their reported hashes differ, reloads an unchanged envelope and proves stable
evidence, hashes the exact `include_bytes!` WGSL source independently, and
checks the envelope marker precedes the shader marker in one startup summary.

## Final verification

- `cargo test --workspace --locked` -- exit 0 across all workspace unit,
  integration, documentation, and compile-fail tests.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` -- exit 0,
  zero warnings.
- `cargo fmt --all -- --check` -- exit 0, no formatting differences.
- `git diff --check` for Task 2 -- exit 0.

No native screenshot was taken because the assigned tranche was explicitly
limited to implementation Tasks 1 and 2. The design retains a fresh `%TEMP%`
GDI screenshot from the stable executable as the final live acceptance gate.

## Self-review

- The Rust mirror and WGSL helper use the same unclamped additive equation.
- `sample_sun` and `sample_moon` retain sampled RGB and only geometric/horizon
  coverage; UV mapping, phase selection, horizon visibility, bindings, and
  pipeline ABI are unchanged.
- Both celestial bodies call one helper. `celestial_opacity` and both
  celestial replacement `mix` paths are absent. Cloud mixing is unchanged.
- Border traversal is deterministic, visits each local border coordinate once,
  distinguishes every moon phase, and rejects unexpected celestial dimensions
  or byte lengths.
- The envelope identity reuses the SHA-256 already computed from validated
  bytes. The shader identity hashes compile-time embedded source bytes and
  performs no source-tree read at runtime.
- Evidence identities contain only lowercase hexadecimal digests. The ordered
  startup line preserves the previously logged asset path but includes no
  payload bytes, credentials, or new machine-specific identity.
- The final diff from the supplied base contains no cloud implementation,
  `plan.md` change, generated asset, or screenshot.

No Critical or Important issue was found during self-review. Independent review
subsequently found and resolved the evidence-line path leak described below.

## Independent-review follow-up

The reviewer found that `LoadedAtmosphereAssets::startup_summary()` included
`selected_path().display()`. That path is machine-specific, so otherwise
identical evidence changed with the checkout or temporary directory.

TDD RED:

- Added `atmosphere_evidence_summary_contains_only_stable_hashes`, which loads a
  synthetic atmosphere carrier from a path containing an explicit local-path
  sentinel, rejects the complete selected path in the summary, and asserts an
  exact ordered two-hash evidence record.
- `cargo test -p bedrock-client --test assets atmosphere_evidence_summary_contains_only_stable_hashes --locked -- --exact`
  failed as expected with `selected path leaked` and printed the `%TEMP%` path
  from the old summary.

TDD GREEN and verification:

- `startup_summary()` now emits exactly
  `ATMOSPHERE_EVIDENCE envelope_sha256=<64 lowercase hex> shader_source_sha256=<64 lowercase hex>`.
- The pre-existing human-readable selected-path diagnostic remains a separate
  startup line and is not part of the evidence identity record.
- The exact focused regression passed: 1 passed, 0 failed.
- `cargo test -p bedrock-client --test assets atmosphere --locked` passed:
  8 passed, 0 failed.
- `cargo test -p bedrock-client --test assets --locked` passed: 38 passed,
  0 failed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` passed with
  zero warnings.
- The first `cargo fmt --all -- --check` identified two formatting-only diffs;
  `cargo fmt --all` applied them, after which the exact regression,
  `cargo fmt --all -- --check`, and `git diff --check` all passed.
