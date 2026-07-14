# Exact visible-quad model drawing report

Status: implementation ready for review.

Implemented:

- Added the exact 8-byte `PackedModelDrawRef` stream while preserving 16-byte
  `PackedModelRef` and 8-byte template-ordered lighting records.
- Meshing drops fully hidden model instances and emits deterministic ascending
  draw refs for each visible mask bit.
- Shared-arena layout, bounds, accounting, COW/retirement, upload patching,
  direct/MDI ranges, expected streams, and witness paths carry the new stream.
- CPU upload validation rejects malformed/nonadjacent triplets, out-of-range
  model indices/quads, unset mask bits, and unreachable lighting before commit.
- WGSL now launches six indices per exact visible-quad instance, loads the
  referenced model record, and retains defensive global/template guards.
- Model witness counts remain based on model references rather than draw refs.

Verification:

- `cargo test -p render --all-targets --no-fail-fast`: PASS (104 library,
  53 mesh, 49 plugin tests plus all other render targets).
- `cargo test -p bedrock-client --all-targets --no-fail-fast`: PASS
  (190 main, 23 assets, 14 camera tests).
- `cargo clippy -p render -p bedrock-client --all-targets -- -D warnings`:
  PASS.
- `cargo fmt --all -- --check`: PASS.
- `git diff --check`: PASS.
- Model WGSL parse/validation and shared-binding tests are included in the
  green 49-test render plugin target.

Open: independent code review, release build, and live model/vine performance
A/B against the recorded p50 40.3 ms / p99 47.8 ms / mutation 138.9454 ms.

## Important review fixes

- Upload validation now resolves each model ref against the extracted immutable
  template table, rejects zero masks and out-of-template mask bits, requires a
  zero first lighting base, and requires template-sized lighting ownership to
  be exactly contiguous with no overlap, gap, or trailing unreachable record.
- Independent regressions cover duplicate bases, first and middle gaps,
  cross-model overlap and draw reads, trailing lighting, and a zero-mask ref
  beside a drawable ref. A valid partial mask still owns all six template
  lighting records.
- The production queue witness count now goes through a tested helper based on
  `model_range / 4`; a 2-ref/10-draw fixture publishes exactly two model refs.
- MDI regression coverage proves three eligible model allocations produce
  exactly three indirect commands with their exact instance counts.

Review-fix TDD and verification evidence:

- RED: ownership regressions failed to compile against the former three-input
  validator; the production witness regression failed because the queue count
  helper did not yet exist.
- `cargo test -p render --all-targets --no-fail-fast`: PASS (114 library,
  53 mesh, 49 plugin tests plus all other render targets).
- `cargo test -p bedrock-client --all-targets --no-fail-fast`: PASS
  (190 main, 23 assets, 14 camera tests).
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  PASS.
- `cargo fmt --all -- --check` and `git diff --check`: PASS.
