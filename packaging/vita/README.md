# Native Vita port — incomplete

The requested deliverable is a standalone, playable Cinnabar VPK with Minecraft:
PlayStation Vita Edition controls. This directory does **not** deliver it yet.
The Bedrock version and protocol remain owned by `assets/bedrock-target.json`.

## Implemented boundary

`client` consumes the existing palette world and mesher rather than inventing
another voxel engine. Cube quads become indexed, bounded uploads when published;
unchanged frames use GPU-mapped streams, without expanding/copying all vertices.
The native ABI selects mapped main RAM explicitly and releases ownership only
after GPU completion. The initial completion wait is blocking and still needs a
nonblocking GXM retirement path before streaming performance can qualify.

The native player controller advances the shared movement simulator at its fixed
tick rate, interpolates the camera, retains jump edges between ticks and clears
pending input when a menu takes control. Cube uploads split by material and index
capacity. Base RGBA texture layers upload once and retain their memory admissions
until GPU completion. Mips, tint, alpha modes, animation and model/liquid materials
are still absent; the developer material path rejects unsupported flags.

`controls` feeds the existing semantic router. The provisional Legacy Console
layout is: left stick movement, right stick look, X jump, R attack, L use/place,
left/right directions hotbar, down sneak toggle, up perspective, Triangle inventory,
Square crafting, Circle drop, START pause, SELECT player list. Inventory/crafting/
drop are shell requests pending integration. UI focus separates confirmation from
gameplay and quarantines held controls. Sprint uses double-forward; the exact
350 ms window and sneak semantics remain provisional until a retail Vita capture
is compared. Touch inventory/hotbar input and rebinding are not implemented.

The launch-era control description was checked against the hands-on
[Vita review](https://reviews2go.home.blog/2014/10/21/minecraft-review/). It is not
proof of every later retail revision. A versioned native controls capture is still
required; modern PlayStation Bedrock bindings are not a substitute.

## Build and verification

Install Rust `nightly-2026-10-22` with `rust-src` and `rustfmt`, and a **hard-float**
VitaSDK with the `vitaGL` package. The native ABI is incompatible with the softfp
SDK used for Android shared-library wrappers. Development used VitaSDK `2026.08.1-1`
and `vitaGL 0.0.0.r1488.g2bdbe89-1`.

The mapped draw API requires a separately built VitaGL with its legacy pipeline
enabled. Its default package can export a no-op `vglDrawObjects`. The developer
check also disables the library's separate startup splash thread. Use the native
SDK's compilers on `PATH` and the unmodified VitaGL source revision
`2bdbe895ddc6a7e410a9ea55a8f53d8173c61ad8` from
[Rinnegatamante/vitaGL](https://github.com/Rinnegatamante/vitaGL). Keep this dependency
checkout outside Cinnabar. `-B` rebuilds objects when changing build options:

```powershell
make -C C:/path/to/vitaGL -B -j4 NO_SPLASHSCREEN=1 HAVE_VITA3K_SUPPORT=1 ENABLE_LEGACY_PIPELINE=1
```

```powershell
./packaging/vita/build.ps1 -Mode Test
./packaging/vita/build.ps1 -Mode Check -Sdk C:/path/to/vitasdk-native
./packaging/vita/build.ps1 -Mode RenderCheck -Sdk C:/path/to/vitasdk-native -VitaGlSource C:/path/to/vitaGL
./packaging/vita/build.ps1 -Mode WorldCheck -Sdk C:/path/to/vitasdk-native -VitaGlSource C:/path/to/vitaGL
```

The script uses a dedicated build directory outside the checkout and `target-vita`
for artifacts, with incremental compilation disabled. If the SDK lacks the host
`vita-make-fself` utility, supply its existing executable with `-FselfTool`; target
compilation and libraries must still use the hard-float SDK. The script supplies
simd-adler32's missing ARM NEON nightly gate only to
that dependency. It does not disable NEON across the client or change cached
dependency sources. `assets`' rustix dependency is restricted to the platforms
actually using its atomic publication API; the Vita fallback still returns
`Unsupported` instead of falsely claiming atomic file publication works.

`RenderCheck` builds an explicitly named **developer rendering check**, with
diagnostic assets and a synthetic palette subchunk. It tests native linking and
the mesh/input boundary; it cannot join, create, load or play a Bedrock world.
It is not the requested release VPK. It writes
`ux0:data/cinnabar/render-check.log`; START+SELECT exits the example.
Hardware execution and rendered-frame inspection remain required even if it links.

`WorldCheck` is an interactive **integration fixture**, not a local Minecraft
server. It connects shared movement, collision, picking, mutation and meshing in
one synthetic section. X jumps, the sticks move/look, R breaks, L places, START
pauses, and START+SELECT exits. Its diagnostic reticle is not the game HUD. A
subchunk checkpoint verifies disk round trips; it is not a Minecraft world save.
Inventory, crafting and gameplay rules are not implemented in this example.

The fixture can load the locally prepared world carrier from
`ux0:data/cinnabar/assets/`, checking its seal and pinned provenance, resolving
its air/bookshelf identities, and drawing its per-face textures. Missing files
select an explicitly logged diagnostic fixture; invalid files fail the check.
This exception is confined to the developer example. A game entry point still
needs the complete required startup carrier gate.

For an automated emulator check, use a separate Vita3K executable directory and
data directory. Vita3K writes its log beside the executable, so two runs sharing
that directory can overwrite each other's evidence. Install firmware and the
shader compiler into the private test filesystem; none is bundled in the VPK.
Set `pref-path` explicitly in the test config. For image inspection use Vulkan,
`disable-surface-sync: false`, `async-pipeline-compilation: false`, and
`memory-mapping: external-host` on a GPU supporting that mode. With the tested
Vita3K `4218-5844e21f`, double-buffer mapping returned zero pixels and page-table
mapping crashed during startup. These are emulator observations, not Vita results.

```powershell
./packaging/vita/test-emulator.ps1 -Emulator C:/test/vita3k/Vita3K.exe -Config C:/test/config.yml -VitaFs C:/test/fs
./packaging/vita/test-emulator.ps1 -Check World -WorldCarrier .local/assets/compiled/vanilla-v2193.mcbea -Emulator C:/test/vita3k/Vita3K.exe -Config C:/test/config.yml -VitaFs C:/test/fs
```

The harness creates `ux0:data/cinnabar/render-check.auto`, because the Rust Vita
target does not expose command-line arguments. The example captures a presented
frame, rejects a uniform image, runs 120 frames, and exits. The harness archives
old outputs, enforces a timeout, saves logs/capture and records the VPK hash under
`target-vita/emulator`. Guest success, image creation and a clean emulator exit
are separate requirements; a shutdown crash is not silently accepted. Remove a
manually created `.auto` marker to return the example to interactive mode.

For `-Check World`, the marker is `world-check.auto`. It drives a deterministic
simulation clock and scripted pad samples inside the guest for 300 frames. It
asserts walking, jumping, break/place, checkpoint reload and changed captured
pixels. The reticle alone cannot satisfy the nonuniform-image check. This is a
functional test; it does not establish real controller input, frame pacing or
hardware performance. `-WorldCarrier` copies and hash-verifies the local carrier
in the isolated emulator filesystem; game assets are not included in the VPK.

The initial emulator run with external-host mapping completed 120 frames and
produced an inspected 960×544 image of the untextured subchunk. The interactive
fixture also completed its scripted checks with inspected diagnostic and pinned
Minecraft bookshelf/plank textures. Twenty-one focused host tests pass. Vita3K
then raised an access violation during
shutdown, after reporting guest exit code 0: the overall emulator test therefore
**fails**. No full-game, visual-parity or hardware performance gate is closed.

## Memory policy

Physical RAM is not a process allocation budget. Start with a 128 MiB newlib heap,
a 48 MiB mapped graphics RAM pool, a separate 96 MiB CDRAM pool, no PHYCONT/CDLG
pool, and a 2 MiB circular pool. Disable VitaGL's automatic spill into newlib.
These are unqualified starting allocations, not measured safe maxima.

Admission currently bounds **tracked payloads**, not every allocation in the
application or SDK. Main-memory payloads have a combined 160 MiB ceiling;
CDRAM's ceiling is 96 MiB. A lease stays alive for CPU staging, queued work and
submitted GPU resources. A replacement needs capacity while the old resource is
still alive. Failure returns backpressure/allocation errors instead of exceeding
the ledger or truncating valid world data. The cube upload is at most 4096 quads,
with 16-bit indices. It reserves before allocating and uses fallible reservations.

Still required: enforce owner budgets throughout assets/world/session/UI/audio,
including the developer fixture's one-shot shared asset decoder, whose internal
tables and textures are not yet charged to the Vita payload ledger. The file
staging buffer is admitted, but the logged peak is not total application memory.
Also measure allocator overhead and SDK ring/render-target allocations, cancel stale
chunk work, evict rebuildable resources, and stream carriers/audio in bounded
windows. Do not decode whole music tracks into RAM. Current desktop audio helpers
can materialize full PCM and cannot simply be called without a Vita streaming
adapter. Long sessions, dimension changes, replacement packs, suspend/resume,
and concurrent old/new mesh lifetimes need real device high-water measurements.

## Full-game acceptance remains open

| Area | What must work on the Vita |
| --- | --- |
| Standalone core | Authentication, encryption, RakNet/NetherNet, Realms/friends, resource packs, reconnect and cancellation. The existing Go core has no Vita port here. No external PC service qualifies as the standalone deliverable. |
| Local worlds | Creation, load/save/reload, world generation, entities, ticks, inventory/crafting, combat, redstone and dimension travel. The current local server is Go/Dragonfly, not Vita-ready. |
| Rendering | Complete materials, models, liquids, light/AO, biome tints, entities, animation, particles, atmosphere, UI, fonts and resource packs. The native adapter currently draws opaque cubes with static base textures. |
| Client composition | Integrate shared gameplay/session owners without the Bevy/wgpu/winit desktop render path. Desktop process spawning and Unix socket assumptions require a native host design. |
| Device services | Audio streaming/mixing, IME, front touch, persistent settings/saves, network lifecycle and suspend/resume. |
| Controls | Retail Vita layout in gameplay, inventories and crafting; all held/release transitions, focus, touch and rebinding. |
| Performance | Sustained controllable play, bounded loading, correct frames and measured pacing/memory on hardware. Neither a successful compile nor an interactive renderer demo proves this. |
| Distribution | A game VPK, asset acquisition/validation, licenses, installation/relaunch verification and a complete feature matrix. No completed release is claimed. |

Cinnabar itself is an independent Bedrock-compatible client with open parity
requirements in `plan.md`. A Vita port cannot honestly claim more feature parity
than the shared implementation and its actual Vita integration establish.
