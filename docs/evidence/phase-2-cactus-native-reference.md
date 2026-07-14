# Phase 2 cactus native reference

Date: 2026-07-14

Protocol: 1001

Native version: Bedrock 1.26.33.1

No Mojang assets or screenshots are tracked by this manifest.

## Native result

- Random tick speed was set to zero.
- `/setblock` wrote every exact `age:int` value 0 through 15 on isolated sand.
- An exact one-block `/fill ... air replace cactus ["age"=N]` probe returned
  one replaced block for every age, after which each block was restored.
- All ages use identical rendering.
- Native visible bounds are X/Z `1/16..15/16` and Y `0..1`.
- West/east/north/south use `cactus_side`, down uses `cactus_bottom`, and up
  uses `cactus_top`. Side U samples source columns 1 through 14.
- One-, two-, and three-block stacks are full height without an air seam.
- Cactus is alpha cutout, not full-face occluding, and does not neighbour-cull
  its inset faces against opaque blocks.

## Local-only screenshot hashes

| Capture | SHA-256 |
|---|---|
| overview | `20e8758114ca8c750a45e460dd3da7b28d00ec354d6e56b64e875dbd2db597c1` |
| stack front | `7a44b58668ffe3b7cac93a5dabe6480bf40b120299d46feb096b2867c0f15b66` |
| stack grazing | `1cd2d8e239e13c468c5491041ef46e70e624fbf015a9910cf34fa193a8c774e2` |
| top inset | `d3780ea4d9bfc346b0b5bf453375e1f37390bd193572f48a6abe4f477a08d321` |
| ages 0-2 readback | `1e8f869e2f87be7ea9b2fe850c7c7d63e305702d430e4de78620d370bfa67abd` |
| ages 3-5 readback | `076bf72d7d5d1545e20f8facf366b1da4bc1137e2dc2f63c29c2e85af7f2d70d` |
| ages 6-8 readback | `fe53fd75e33ccd02e742e5bcbad66c88d0422a17b4b0150c7413238932cf9397` |
| ages 9-11 readback | `7a39b03b099f355634d480f4269c36ad2c47dae46eee1bf376d769ea0e52a240` |
| ages 12-14 readback | `62abbbc1aa5be975b237bc498d1903e126dbdacc46ab56ec5c47fbafc42f89c6` |
| age 15 readback retry | `d4f438ff5010ed6063b8d43dfee443a43b16e62a6ec276a7a87c9ff57115f92c` |

Images remain under `%TEMP%` and are not repository content.

## Generated artifact hashes

| Artifact | SHA-256 |
|---|---|
| `block-registry-v1001.bin` | `23a504f0daa248c717249d0aa247362933ff963754aedd790566fc0516cdcf95` |
| ignored `vanilla-v1001.mcbea` | `ddee460c3bad5d14eb81216dc669389813c6a1a805de398de2b95f56bc87bc7d` |
| `visual-coverage-v1001.json` | `2ee5a68b09ff92e422f5fae9dd49a45b74af29c7b1639d8e8c7f173e90de5509` |

Two registry generations and two pinned-pack compilations were byte-identical.
The exact production ratchet removed only IDs 13,606 through 13,621 with zero
additions, reducing diagnostics from 2,479 to 2,463. The matching-view,
two-frame Cinnabar GPU witness remains open.
