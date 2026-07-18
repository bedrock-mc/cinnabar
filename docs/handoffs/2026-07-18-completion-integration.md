# Completion integration handoff

Branch: `completion-performance-profile`

Base: `origin/phase2-textures` at `d026417`

Status: locally committed integration checkpoint; not yet accepted for merge into
`phase2-textures` because the native actor-render gate remains open.

## Included work

- `4100202` (`f2d9205` equivalent): publish compiled actor rigs through the app.
- `4e3cd1d` (`3150ef8` equivalent): reserve the visible local avatar and preserve
  its exact roster skin.
- `924a655`: record the approved compact-chat exception and prohibit raw JSON,
  placeholder geometry, duplicated HUD elements, and production debug surfaces.
- `db6ed01` was an erroneous broad AGENTS.md replacement and `4db9302` is its
  complete revert. The net `d026417..HEAD` AGENTS.md diff is only the two intended
  policy bullets; history was not rewritten because repository policy prohibits it.

## Verification completed

- `cargo build --locked --release -p bedrock-client`
- `cargo test --locked -p bedrock-client -p client-world -p render`
- `cargo clippy --locked -p bedrock-client -p client-world -p render --all-targets -- -D warnings`
- `cargo fmt --all -- --check`
- `cargo run -p architecture --locked -- check --root . --policy tools/architecture/policy.toml`
- `git diff --check`
- Independent Phase 4.3 review: APPROVE, no findings.

The post-integration run passed 335 app tests plus the client-world/render suites;
the later input-tap branch raises the app count to 336 but is intentionally kept
separate.

## Native evidence and open gate

The exact release client joined LBSG and Zeqa with compiled block, atmosphere,
entity, and font carriers. DX12 Immediate rendered LBSG at roughly 180-217 FPS.
No remote actor appeared in the observed lobby, and the pre-fix automation tap of
F5 was not sampled, so no real actor/skin/animation frame was obtained. Do not
merge this branch into `phase2-textures` until a current native frame proves the
shared rig, exact skin, local-avatar 0/1/1 visibility, geometry, depth, and motion.

The local ignored performance artifacts record Lunar DX12 FIFO at roughly 6-8
FPS versus Immediate at roughly 190 FPS. The source remedy is preserved on the
separate `performance-dx12-present-mode` branch.

