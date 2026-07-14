# Phase 2 farmland native reference

The exact farmland checkpoint used matching Bedrock 1.26.33.1 BDS and native
Minecraft for Windows. Random ticks were disabled and the fixture was kept
away from water and crops. Mojang payloads and screenshots remain local-only.

## Exact state product and fixture

All eight states were placed, destructively exact-read, and restored with:

```mcfunction
setblock <x> 193 <z> farmland ["moisturized_amount"=<amount>]
fill <x> 193 <z> <x> 193 <z> air replace farmland ["moisturized_amount"=<amount>]
setblock <x> 193 <z> farmland ["moisturized_amount"=<amount>]
```

Amounts 0..3 occupied `(196+4*a,193,60)` and amounts 4..7 occupied
`(196+4*(a-4),193,66)`, each above dirt at Y=192. The contrasting platform was
Y=191, X=190..218, Z=52..78, with west/east/north/south borders red, blue,
green, and yellow. The exact UTF-8 layout string is:

```text
platform:y=191;x=190..218;z=52..78;fill=black_concrete;west:x=190:red_concrete;east:x=218:blue_concrete;north:z=52:green_concrete;south:z=78:yellow_concrete;farmland[0..3]:x=196+4*a;y=193;z=60;support=dirt@y192;farmland[4..7]:x=196+4*(a-4);y=193;z=66;support=dirt@y192;geometry:farmland[0]@(212,193,74);support=dirt@(212,192,74);full_dirt_reference@(214,193,74);support=dirt@(214,192,74)
```

Its SHA-256 is
`123199a5932a8f6b0b5fc1fb2551afe2767de1abd11a6a0d950e533b1492d462`.

## Native conclusions

- Amount 0 uses the dry top at terrain-array index 1.
- Amounts 1..7 all use the wet top at terrain-array index 0.
- Every state has full X/Z bounds and height 15/16; moisture does not alter
  geometry.
- Horizontal and bottom faces use dirt. The vanilla screenshot is visually
  consistent with the existing coordinate-derived 15-pixel crop
  (`V=4096..256`), but does not independently prove the exact source-row
  identity versus stretching. The crop remains the audited implementation
  contract; a local-only numbered calibration pack would be required for an
  independent pixel-address proof.
- No selected source is tinted, animated, translucent, or emissive.

The top-selector gallery at
`%TEMP%\\cinnabar-native-farmland-selector-top-20260714.png` has SHA-256
`0ce87ce946308ad92ce5ed3dd2b23a880730a74dcc8f2580b6525f1935084e05`.
The height/side comparison at
`%TEMP%\\cinnabar-native-farmland-height-uv-20260714.png` has SHA-256
`d2d189f45ea091cba9ae3a54cce9e7935737a0428ddc9b2921066da4ff4a8782`.

## Deterministic production result

The exact production ratchet removed only IDs 6,122 through 6,129, with zero
additions: 2,456 diagnostics including air became 2,448. Registry generation
and pinned-pack compilation are byte deterministic.

| Product | SHA-256 |
|---|---|
| accepted post-cake registry input | `050cf1e79f9505cfcb240b1eb6627df95451e062e77b368b6d2700c21e68c3e6` |
| farmland registry output | `e27e6e5775342c1f4089b749c69afeac19937dac0f5b7834c73164f1b6fa442c` |
| refreshed coverage baseline | `999776347737db024fdd523f99aa5e7d2fae45d3288a93b51b79a3e2a5c41ae3` |
| ignored pinned-pack MCBEAS output | `6509eadf24068fc029f04ca67187517e76698bc1e31a8326bdb07e74a0c91f25` |

The matching-view Cinnabar two-frame GPU witness remains part of the later
batched live-presentation gate.
