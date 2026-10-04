# macOS GPU lockup investigation

State: stability acceptance incomplete. The hardened path is retained through
the final 2026-10-03 dev integration; the user is manually testing it. Historical
build/status entries below identify their own snapshots, not the latest state.
This investigation takes priority over feature acceptance. Do not reproduce the
old unchecked/indirect path on the owner's working Mac.

## Observed evidence

Target: Apple M3 Pro, 18 GB unified memory, Mac15,6, macOS 26.3 (25D125).
The failing canonical debug client used Metal and MultiDrawIndirect.

On 2026-10-02, the client's system GPU-event reports repeatedly recorded
`process_name=bedrock-client`, `signature=577`, `guilty_dm=1`, and
`restart_reason_desc=firmware-detected lockup`. The last inspected report was
`gpuEvent-bedrock-client-2026-10-02-053335.ips`.

Four inspected `panic-full` reports at 04:57, 05:09, 05:36 and 05:55 recorded
WindowServer userspace-watchdog failures. The 05:54 watchdog spin showed
WindowServer blocked in AGXG15S and client threads blocked in IOGPUFamily/AGXG15S.
The client footprint was approximately 4.84 GB. This is strong GPU-lockup
evidence, not an ordinary Rust panic; that footprint does not prove a leak.
Raw reports, logs and captures remain private and outside Git.

## Identified hazards and changes

- Installed Bevy 0.18.1 `Shader::from_wgsl` sets `ValidateShader::Disabled`.
  Its pipeline cache consequently creates a trusted shader module with runtime
  checks disabled. Installed wgpu-hal 27.0.4 Metal compilation then permits
  unchecked buffer access. All Cinnabar-owned shader constructors now use one
  private checked constructor, including generated/imported shader modules.
- The biome fragment loop used a storage-buffer word as its iteration limit.
  A bad record address could read header/float bits and create enormous GPU
  work. It now clamps the count to the generated `LATTICE_BIOME_LIMIT`, checks
  spans before offset addition, checks descriptor identity and kernel indices,
  rejects invalid weights, and guards packed palette decoding. Valid vanilla
  samples and constants are unchanged.
- Installed wgpu-hal's Metal multidraw is a CPU loop issuing native indirect
  draws. Metal now selects the existing direct path, avoiding indirect argument
  reads and indirect validation. DX12/Vulkan policy remains unchanged. This is
  isolation, not evidence that indirect draws caused the firmware failure.
- Fresh chunk admission now pauses when removals cannot reserve retirement
  capacity; bounded COW replacements remain eligible. Private NOOP-device
  regressions cover stalled completion, retry/acknowledgement after completion,
  and operation-budget deferral that must not masquerade as retirement pressure.
  Capacity-change logs include device limits, current arena/auxiliary buffers
  and an active migration's temporary capacity, excluding driver retention.

The retirement audit found physical queue-completion fencing before origin/biome
range reuse, bounded sort jobs and retirement queues, and no demonstrated stale
pointer or unbounded COW leak. Adapter buffer limits are not an application-wide
memory budget; aggregate capacity/migration budgeting still needs investigation.

## Verification and remaining gates

CPU regression tests assert checked constructors cannot be bypassed, direct Metal
selection in both debug/release policy, the biome count cap in parsed Naga IR,
guarded address arithmetic, and complete shader parsing/validation. These tests
cannot establish host stability.

Before a controlled live run, rebuild the canonical client, finish offline
checks, stop builds, record binary hash and diagnostic-report baseline, and use
only the hardened draw path. Monitor client memory, arena capacities, frame
completion and new GPU events; stop immediately on a new event or stall. Do not
leave an unattended client running. A short successful run is a smoke test, not
proof that an intermittent whole-host failure is permanently fixed. Prolonged
movement and repeated sessions remain open until recorded.

## Hardened-build smoke witness

Source state: `a1b0e289` plus the uncommitted stability, snow and local-BDS edits.
Server: offline official BDS 1.26.52.3, world `6b0acddbce41f58d`, requested
radius 10; macOS/Metal window 1280x752 logical points, Retina scale 2.

Run: 2026-10-02 11:41:26–11:51:50 UTC. The monitor sampled for about 600 seconds;
every sample showed advancing main-world frame IDs and zero new client GPU
events. Actual W/A/D movement was verified from before/after positions; server
camera corrections exercised yaw 0, +0.70, -0.70 and +1.57 with new chunk loads.
Synthetic mouse events did not change yaw, so this is not physical mouse-turn
acceptance. Fresh native ScreenCaptureKit frames show live terrain, water,
snow, weather and HUD rendering. No validation error or client panic was logged.
Tracked chunk arena capacity reached approximately 63 MB including auxiliary
buffers; reported device storage-binding limits were approximately 4 GB per
buffer, confirming they are not useful aggregate application budgets.

The monitor sent SIGTERM at the planned ten-minute limit. Its post-termination
PID check initially treated a zombie process name as a changed identity and
reported an error; independent process and BDS-disconnect checks confirmed the
client exited. The private helper now recognizes zombies without signalling
them. This was a harness cleanup issue, not a client crash or a new GPU event.

Full workspace tests (including doctests), formatting, strict all-target
workspace Clippy, architecture check, Go core tests/vet and the canonical build
passed. Changes remain local and uncommitted. This successful short run does
not isolate the original trigger or close prolonged/repeated-session stability.

## Dev resync: offline verification only

The worktree is now on upstream `502ee525` plus the same uncommitted safety,
snow and BDS edits. Integration preserves upstream's resource-retention fixes,
opaque-phase reset, selection overlays and Enhanced world/hand/UI separation.
The full workspace suite passes with `CINNABAR_REQUIRE_ENHANCED_GPU=1`, alongside
focused rendering/snow tests, formatting, strict all-target Clippy and architecture.
The offscreen populated GPU test is not a native game, DPI or stability gate.
Runtime carriers and the canonical client were rebuilt.
That executable had not been launched into a live session at that checkpoint.
The game remained closed for that snapshot, and those changes were test-green
uncommitted, not pushed.
