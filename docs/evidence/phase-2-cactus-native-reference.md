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

## Reproducible fixture

The fresh deterministic fixture uses the following exact layout string:

```text
platform:y=191;x=160..184;z=52..78;fill=black_concrete;west:x=160:red_concrete;east:x=184:blue_concrete;north:z=52:green_concrete;south:z=78:yellow_concrete;cactus[a]:x=166+4*(a%4);y=193;z=58+4*floor(a/4);support:sand@y192;a=0..15;adjacency:cactus[age=0]@(180,193,74);support:sand@(180,192,74);east_neighbor:black_concrete@(181,193,74);probe:(180,193,76);probe_support:sand@(180,192,76)
```

Its UTF-8 SHA-256 is
`5eee087fcd4b4b0984812074cbb72c099479447d5296ae1969c381bddefa4351`.
The colored borders make the four cardinal directions identifiable: west red,
east blue, north green, and south yellow.

The fixture was created with random ticks disabled and exact Bedrock commands:

```mcfunction
gamerule randomtickspeed 0
fill 160 191 52 184 191 78 black_concrete
fill 160 191 52 160 191 78 red_concrete
fill 184 191 52 184 191 78 blue_concrete
fill 160 191 52 184 191 52 green_concrete
fill 160 191 78 184 191 78 yellow_concrete
```

For each `a` from 0 through 15, the grid uses
`x = 166 + 4 * (a % 4)`, `z = 58 + 4 * floor(a / 4)`, sand at Y=192,
and this exact state at Y=193:

```mcfunction
setblock <x> 192 <z> sand
setblock <x> 193 <z> cactus ["age"=<a>]
```

The independent fixed-coordinate state probe at `(180,193,76)` was reset,
written, and read back for every age using:

```mcfunction
setblock 180 193 76 air
setblock 180 193 76 cactus ["age"=<a>]
fill 180 193 76 180 193 76 air replace cactus ["age"=<a>]
```

Every exact readback returned `1 blocks filled`. The opaque-neighbor fixture is
an age-zero cactus at `(180,193,74)` on sand with black concrete immediately to
its east at `(181,193,74)`. It was written with the exact command below and
visually captured both with the placement result visible and after the overlay
faded:

```mcfunction
setblock 180 192 74 sand
setblock 181 193 74 black_concrete
setblock 180 193 74 cactus ["age"=0]
```

## Local-only screenshot hashes

| Capture | SHA-256 |
|---|---|
| deterministic overview | `74fe9fee1bae06d22c69d988948ad209eda106df24e278861f920a701ec74d89` |
| opaque-neighbor placement | `42dd93d0e5077dfe88ec85eaf809a96e59e7f8d0ff9d283ca30ff5652dbe479f` |
| opaque-neighbor clean view | `885bc4ef88c0efe70773caa3ad67717cdeee75e5d346d25f4dfe425a5771d2da` |
| exact age 0 readback | `db3f01435a40d0c2a923f4adae6fa226a517da21a6500cc4f3c940f1e2978433` |
| exact age 1 readback | `e857fc0d892c0a4eb1ca110ced84dcfc8a32063a20d83e5378338885005ddbd8` |
| exact age 2 readback | `56036646c70a9786a05f0cd474f3692f06f513bf4e44e3c5099543d547645c64` |
| exact age 3 readback | `69ffb234e948e18fbbe46f10f9650f22d7ebfcfc606e71d3c451b2164ef07e24` |
| exact age 4 readback | `50221c1adf3125f0beed7262ebe013352841902ebaf44bb5ba7fdd59417a9aac` |
| exact age 5 readback | `6aa5a01ca01e11cb578d5dbc2cae0d635df6e06773af877eed2fadc6693c3fe6` |
| exact age 6 readback | `bfac43286d103a740ad680261bf37dacf77217638a886c9aa871f716f9930b4c` |
| exact age 7 readback | `d1dec9f18bebccf46931108d99076c6a3e9e6e855be07c535cfefb91662f0714` |
| exact age 8 readback | `d93e9f5184734e2da8b9312a74aca73a44da07a8918b931252a37a5e6ec367c1` |
| exact age 9 readback | `cbecba6825dc8b2a140bdc2431a829c17a2dc1b454c2451e13b755263ada54df` |
| exact age 10 readback | `d88be71ab31a135e67839e0eb4a29c1946a4ea18a82ecbbfc31910c568f01522` |
| exact age 11 readback | `7ac69d1ec2f9e0af55dd5c78836166661ad6a008594a7dac6639a4f62be3a858` |
| exact age 12 readback | `8fdd04b78bbb5337e92f1a97074b112f373d9d39a930918e3cc420efe39f973a` |
| exact age 13 readback | `03e5412b19fff5eed7624404370697f4045e65d655f1ba38df202524e54585de` |
| exact age 14 readback | `6b36fb5ba1db54baf946fdc44855aca63edc1db27143eccb683934ccc9997ef1` |
| exact age 15 readback | `b40c433648573eb15ea271710851becc8206cd0583b4d39a9c7561eb16da8675` |

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
