# Local world generation

Dragonfly's Normal generator uses vanilla-gen for all three dimensions. Its current parity
fixtures target Java-style terrain, not Bedrock seed parity. Bedrock terrain, biome placement,
structures, loot and mobs remain incomplete; these measurements close no parity or release gate.
The pinned update keeps noise and spline rounding consistent across CPUs without relaxing tests.

## Pre-generation

Use `bedrock-local-server -dir <world> -addr 127.0.0.1:<port> -generator normal -seed <seed>
-pregen-radius <chunks>`. The inclusive square surrounds the saved overworld spawn, or the
new generator spawn. Missing columns are saved before `ready`; existing columns are preserved.
Re-running resumes work. Zero disables the option; Flat and synthetic fixtures reject it.
Keep stdin open until ready; `stop`, stdin EOF or an interrupt cancels unfinished generation.
`-chunk-workers` controls generation parallelism (default four, range 1–16). Progress includes
saved/existing counts and generation-plus-storage throughput. `-generation-stats` reports total
generation work by dimension at shutdown; its rate includes idle time and is not peak capacity.

## CPU measurements

M3 Pro, macOS arm64, Go 1.26.2, low-priority processes, fresh databases, 81 columns around spawn.
One sample per seed and worker count, with other development workloads present. The generator's
OpenCL backend supports Linux only; macOS takes the reported CPU fallback.

| Seed | One worker, chunks/s | Four workers, chunks/s | Eight workers, chunks/s |
| --- | ---: | ---: | ---: |
| 1 | 18.03 | 54.16 | 76.38 |
| 42 | 11.60 | 37.51 | 46.50 |
| -7 | 19.40 | 51.08 | 65.92 |

Four workers improve these matched generation-plus-storage samples by 2.63–3.23× over one.
Eight offer more throughput but use more of the owner's CPU; four is the modest default.
Full generator tests pass with both upstream Dragonfly and Cinnabar's pins. Optional external
region fixtures and Linux OpenCL tests report named skips. Generated palettes resolve against
the current registry in all three dimensions; this is compatibility evidence, not terrain parity.

A hidden 1920×1080 client with a 12-chunk render distance joined a fresh seed-1 server. From the
MCP connect call (including server startup), one worker took 2.46 s to spawn and 20.09 s to load
the eight-chunk square. Four took 1.13 s and 7.73 s. These single observations are neither p99s nor
complete-visible-terrain timings. Seed 42's eight-chunk-square check timed out after 60 s.
The client build was the existing developer-control play build at `3abbe9f94`.

A larger seed-1 pre-generation saved 4,225 columns in 129.55 s (32.61/s). The following headless
join generated zero columns, spawned in 1.11 s and loaded the eight-chunk square in 7.22 s.
During a short flight the late samples moved at about 125–129 blocks/s, while resident columns
fell from 491 to 306. This isolates a further bottleneck after generation, not specifically the
client: storage, lighting, networking and client decode/render still need separate attribution.
Surface stills for seeds 1, 42 and -7 show coast/ocean and forest terrain. Underground capture
is diagnostic and does not establish cave rendering parity. The seed-1 saved region contains
normal and deepslate ores, oak/birch tree blocks, planks and chests; structure placement/loot
were not compared with matching Bedrock ground truth.

## Remaining performance work

At 128 blocks/s and radius 12, a straight route requests roughly 200 new columns/s before
prefetch; the required twice-arrival capacity is roughly 400/s. The measured four-worker capacity
is 38–54/s, leaving a substantial supply gap. The live speed-12 route also lost resident columns
while flying. Docker's daemon was unavailable, so no BDS throughput comparison was measured and
no faster-than-BDS claim is made. BDS remains the official generation reference when available.

Ranked follow-up opportunities:

1. Profile cold base terrain and biome/climate evaluation; those dominate uncached work.
2. Profile cross-column decoration and structure planning, then remove repeated neighbor work.
3. Batch generation and bound shared-cache memory before increasing parallelism further.
4. Evaluate a Metal compute backend or the existing Linux OpenCL backend on supported hardware.

Captures, raw logs and databases stay outside git under `/private/tmp/cinnabar-localworld-evidence`.
