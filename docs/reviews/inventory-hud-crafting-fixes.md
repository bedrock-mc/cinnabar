# Selected-item label, restacking and crafting corrections

This is scoped acceptance for the reported regressions, not closure of the
overall inventory/native-parity gate. Behavior references and remaining gaps:

- [Selected-item HUD label](../reference/selected-item-hud-label.md)
- [Bare block stack identity](../reference/inventory-block-restacking.md)
- [Recipe admission](../reference/inventory-recipe-admission.md)
- [Crafting close conservation](../reference/inventory-crafting-close.md)

## Integrated BDS acceptance, 2026-10-02

Canonical executable: `target/debug/bedrock-client`, built from `48038ec0` plus
these corrections.
Test run: 09:03–09:10 UTC. macOS 26.3, Apple M3 Pro, Metal, debug profile,
Retina display scale 2; logical gameplay area 1280×720, framebuffer 2048×1152.
No performance acceptance is claimed.

Official BDS, managed by the existing core, remained running throughout; world
`a8a7407478460cc7` was not regenerated. Endpoint: loopback
`127.0.0.1:56399` (Docker UDP mapping to the normal BDS port). The installed
server payload reports `1.26.52.3`, with the game's matching protocol target
selected by the repository manifest. `online-mode=false`; the client connected
without Xbox credentials. The test player was `BowTest` in survival.

Fresh Orca app/window discovery and native capture established the target. The
native keyboard route had a current focus integration failure, so the existing
PID/frontmost-guarded `.local/dev/live-input` helper supplied clicks/keys and
requested new client F2 captures. Each relevant frame was inspected. Input
receipts alone were not considered success.

| Scenario | Wire/server result | Inspected local frame |
| --- | --- | --- |
| Dirt 64 → cursor 32 → restack 64 | Take `-3`, Place `-5`, both Accepted; independent BDS `hasitem` query found exactly 64 dirt | `.local/screenshots/2026-10-02_09.03.50.png` |
| Eight logs in personal grid | Four-plank result visibly available; grid placement `-9` Accepted | `.local/screenshots/2026-10-02_09.04.07.png` |
| Craft once, place output, close/reopen | Craft `-11`, output Place `-13`, leftover-input return `-15`, all Accepted; BDS confirmed dirt 64, logs 7, planks 4 | `.local/screenshots/2026-10-02_09.04.40.png` |
| Four planks distributed across the 2×2 grid | One crafting-table result; Craft `-27` and output Place `-29` Accepted; BDS confirmed one table, seven logs, zero planks | `.local/screenshots/2026-10-02_09.05.50.png` |
| Workbench recipe shifted into bottom-right cell | Craft `-35`, output Place `-37`, leftover-input return `-39`, all Accepted; reopened workbench empty with logs 6 and planks 4 retained; BDS independently confirmed these counts and the table | `.local/screenshots/2026-10-02_09.08.30.png` |
| Close workbench while carrying half the dirt | Take `-41`, close-time merge `-43`, both Accepted; personal inventory reopened with dirt 64 and empty cursor | `.local/screenshots/2026-10-02_09.08.51.png` |

The workbench fixture was placed with BDS `setblock ... keep` at `(12,64,11)`
after an independent `testforblock ... air` check; no occupied block was
overwritten. The test changed weather/time for visibility, exercised creative
HUD layout, then restored survival. It did not reset player/world data.

HUD frames `.local/screenshots/2026-10-02_09.07.12.png` (survival),
`2026-10-02_09.09.37.png` (creative), and `2026-10-02_09.10.14.png`
(survival restored) show the selected name centered above the hotbar rather
than inside its slots. White text and shadow remain legible against the scene;
the survival status rows retain clearance. No clipping, incorrect layering or
unexpected background was visible at the tested scale. Inventory hover tooltips
remain separate from the transient selected-item name. Repeated live
close/reopen and subsequent gestures retained working focus/input.

Raw local evidence: `.local/dev/inventory-final-live.log` and the BDS Docker
log. Screenshots, runtime payloads and logs are deliberately excluded from git.

## Regression coverage

Focused ledger tests cover accepted/dependent block restacking, mismatched
aux/block identity, personal/workbench close, partial-stack/empty-cell returns,
overflow Drop, cursor overlays, refusal recovery and reopen. Protocol fixtures
cover discovery metadata and high-bit result identity; JSON-UI checks cover
selected-name factory geometry in survival and creative.

The complete workspace suite, strict all-target clippy, core Go tests/vet,
formatting and architecture checks passed before the upstream refresh. That
workspace run reported 2,183 app tests passed and 18 explicitly ignored tests.
Post-refresh verification and the canonical rebuilt-client smoke witness are
recorded separately below.

## First upstream refresh and verification

Fast-forwarded `dev` to `c3ad062e`, incorporating five upstream commits without
overlapping these corrections. The first post-refresh compile exhausted disk
space in generated Rust caches. Cargo's package-scoped cleanup removed 25.7 GiB
of client build files; retries used `CARGO_INCREMENTAL=0`, the worktree's own
`target`, the shared two-slot limiter and six build jobs. No source, assets or
world files were deleted.

The disk event also ended the old BDS session with a Worldcorruption disconnect
and the old client aborted during cleanup. This is not included in the successful
scenario run above, nor claimed as graceful-disconnect acceptance. Before
recovery, the existing world was copied to `.local/bds-recovery-lDuPX6/world`.
Docker's stopped engine required restarting a hung backend; there was no factory
reset. The original world subsequently opened successfully, retaining the dry
test clearing and its table, independently confirmed by BDS `testforblock`.

These commands completed with exit code 0 on the combined source tree:

- Focused ledger, protocol and JSON-UI HUD regressions, also exercised by the
  final full `cargo test --workspace --locked` run. The app suite reports 2,186
  passed, zero failed and 18 explicitly ignored tests.
- `cargo fmt --all` and `cargo fmt --all -- --check`.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- `cargo run -p architecture --locked -- check --root . --policy tools/architecture/policy.toml`.
- `go test ./core/...`, `go vet ./core/...`, and `git diff --check`.
- Final `cargo build -p bedrock-client --bin bedrock-client --locked`, after
  the workspace test run, at the canonical executable path.

Local logs: `.local/dev/inventory-synced-workspace-retry.log`,
`inventory-synced-clippy-retry.log`, `inventory-synced-architecture.log`,
`inventory-synced-go-tests.log`, `inventory-synced-go-vet.log` and
`inventory-synced-canonical-build.log` in the same directory.

## Rebuilt-client live witness after the first refresh

Same platform, scale, world and BDS payload as above; the recovered container's
endpoint is `127.0.0.1:61526`. Both actual `online-mode=false` and
`allow-list=false` were read from its server properties, and the core recorded
offline authentication. New native app/window discovery and capture were
performed; the keyboard route still reported `window_not_focused`, so the
guarded helper supplied the live gestures. The client joined without credentials.

09:28–09:33 UTC repeat:

- Dirt split/restack requests `-3`/`-5` Accepted.
- Personal Craft `-11` Accepted. Closing while still holding its four-plank
  output returned both seven logs (`-13`) and cursor output (`-15`), all
  Accepted. BDS independently confirmed dirt 64, logs 7, planks 4.
  Inspected `.local/screenshots/2026-10-02_09.29.15.png` shows the empty
  reopened grid, empty cursor and those three inventory stacks.
- The complete four-slot recipe preview was inspected in
  `2026-10-02_09.30.10.png`. Craft `-27` and close-time cursor return `-29`
  Accepted; BDS confirmed one table, seven logs, zero planks.
- Shifted bottom-right workbench preview inspected in
  `2026-10-02_09.30.47.png`; Craft `-35` Accepted. Close returns `-37`/`-39`
  Accepted. Reopened workbench in `2026-10-02_09.31.15.png` has no ingredients
  or cursor item, with six logs and four planks in the player inventory.
  Independent BDS queries confirmed all four inventory quantities.
- Cursor split followed by close (`-41`/`-43`, Accepted) and personal reopen
  inspected in `2026-10-02_09.32.10.png`: dirt 64, cursor empty, subsequent
  inventory opening still functional.
- Creative HUD inspected in `2026-10-02_09.33.03.png`; survival restored and
  inspected in `2026-10-02_09.33.29.png`. Selected text remains centered and
  legible, with proper hotbar/status-row clearance, shadow, scale and layering;
  no clipping or unexpected background. The live log contains no rejected
  inventory response, close-return warning, error or panic during this repeat.

Raw evidence: `.local/dev/inventory-synced-live.log`,
`.local/dev/inventory-synced-core.log` and the recovered BDS Docker log.

## Final upstream refresh and canonical-build witness

Fast-forwarded to `a850497a`, bringing in two further upstream loading-screen
commits without conflicts. A subsequent fetch confirmed no additional upstream
commits before committing these corrections. On this combined tree, all of the
verification commands listed above completed with exit code 0 again. The full
workspace run reports 2,192 app tests passed, zero failed and 18 explicitly
ignored tests. Local logs share the prefix
`.local/dev/inventory-loading-synced-` with suffixes `tests.log`, `clippy.log`,
`architecture.log`, `go-tests.log`, `go-vet.log` and `build.log`.

The canonical client was built after workspace verification, then launched
against the same official offline BDS/world.
Fresh native discovery and capture established the new client window; the same
focus integration failure required the guarded input helper. Platform, debug
profile, display scale and endpoint are unchanged.

09:48–09:50 UTC final-build smoke:

- Split/restack `-3`/`-5` Accepted; inspected
  `.local/screenshots/2026-10-02_09.49.03.png` shows dirt 64 and empty cursor.
- Personal recipe preview inspected in `2026-10-02_09.49.19.png`.
  Craft `-11` Accepted; close while holding the output returned the seven
  remaining logs (`-13`) and four cursor planks (`-15`), both Accepted.
  Reopened inventory in `2026-10-02_09.50.13.png` has empty grid/cursor and
  dirt 64, logs 7, planks 4. The independent BDS `hasitem` query confirmed
  those exact quantities.
- Inspected survival HUD in `2026-10-02_09.50.45.png`: selected Dirt text is
  centered above the status rows/hotbar, not inside the slots; shadow, scale,
  layering and clearance remain correct, without clipping or an unexpected
  background. This complements the earlier creative and workbench witnesses.
- No rejected inventory response, close-return warning, error or panic occurred
  in this final smoke. Raw client evidence:
  `.local/dev/inventory-loading-synced-live.log`; core offline-auth confirmation
  remains in `inventory-synced-core.log` and counts in the BDS Docker log.

The final rebuilt client was left running in survival on this world for handoff.
