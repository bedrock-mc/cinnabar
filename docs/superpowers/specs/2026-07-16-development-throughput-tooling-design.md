# Development Throughput Tooling Design

## Goal

Shorten Cinnabar's normal edit-check-review loop without weakening the full cross-platform gate.
The tooling must preserve the canonical checkout's stable Windows executable path, the existing
per-worktree Cargo target discipline, and the repository's deterministic acceptance contracts.

## Selected approach

The change has four bounded parts:

1. Expand the existing Windows CI job beyond PowerShell contract tests to install the pinned Rust
   and Go toolchains and run the Rust workspace tests, strict all-target Clippy, Go tests, and Go
   vet. Live BDS and GPU acceptance remain local because they require approved executable paths,
   ignored Mojang assets, and interactive machine state.
2. Add a cross-platform Rust development tool that maps changed paths to owning workspace packages,
   closes that set over reverse path dependencies, and runs format, check, test, and strict Clippy
   for the resulting packages. Changes to workspace-global build inputs fall back to the full
   workspace gate. The full CI gate remains authoritative.
3. Add a separate `cargo-fuzz` workspace with initial targets for the pure `RuntimeAssets` blob
   decoder and Bedrock `SubChunk` decoder. A scheduled/manual CI job runs each target with explicit
   time and input-size bounds; pull requests do not pay the fuzz build cost.
4. Benchmark `cargo-nextest` against the existing Rust test runner on this checkout. Adopt it only
   if the measured warm run is materially faster and completes the same test inventory without
   introducing repository-specific concurrency exceptions. This measurement does not replace the
   canonical `cargo test` gate by default.

Faster linker settings are deliberately excluded. They are host-specific, and changing them would
risk the stable Windows live-test executable workflow for an unmeasured gain.

## Affected-package verifier

The verifier accepts an explicit Git base and obtains changed tracked and untracked paths from the
working tree. It uses Cargo metadata rather than a handwritten package map. A changed file belongs
to the package with the deepest manifest directory containing that file. Reverse path dependents
are included because changing a library can break its workspace consumers.

The following inputs require full-workspace verification: the root `Cargo.toml`, `Cargo.lock`, the
Rust toolchain file, Cargo configuration, CI configuration, and unknown paths that can affect more
than one package. Documentation-only changes produce formatting/architecture checks but skip Rust
package compilation. A dry-run mode prints the selected packages and exact commands so selection
logic can be tested without invoking Cargo builds.

Selection and command construction are pure functions covered by unit tests. Git and Cargo command
failures are returned with their command context and a nonzero exit status.

## Fuzzing contract

Fuzz targets call only public, bounded, deterministic decode APIs and treat every `Result` as a
valid outcome. A finding is a panic, abort, timeout, excessive allocation, or sanitizer failure.
The initial input cap is 1 MiB, with shorter scheduled runs intended to find structural decoder
bugs rather than exhaustively explore asset-sized valid payloads.

Corpus, crash artifacts, generated binaries, and Mojang data remain ignored. Reproducing inputs
that uncover bugs become ordinary small regression fixtures in the owning crate before a fix lands.

## Verification

The implementation closes with focused unit tests for affected-package selection, compilation of
both fuzz targets, the existing architecture policy check, formatting, full workspace tests, strict
workspace Clippy, Go tests/vet, and the existing Bash acceptance-harness tests. The Windows CI YAML
is reviewed to ensure it uses Windows PowerShell 5.1 for the acceptance contract and does not launch
BDS or the Bevy client.
