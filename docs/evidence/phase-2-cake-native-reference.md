# Phase 2 cake native reference

The exact cake checkpoint used matching Bedrock 1.26.33.1 BDS and native
Windows Bedrock on 2026-07-14. Screenshots and Mojang payload remain local and
are not repository content.

The complete typed product was placed at `(126,192,55+3*b)` with:

```text
setblock 126 192 <z> cake ["bite_counter"=<b>]
fill 126 192 <z> 126 192 <z> air replace cake ["bite_counter"=<b>]
```

Every `b=0..6` exact replacement returned `1 blocks filled`. The canonical
fixture layout was:

```text
platform:y=191;x=118..134;z=52..78;fill=black_concrete;west:x=118:red_concrete;east:x=134:blue_concrete;north:z=52:green_concrete;south:z=78:yellow_concrete;cake[b]:x=126;y=192;z=55+3*b;b=0..6
```

Its UTF-8 SHA-256 is
`25afee45a35f5fcabca39536d8ef48e3fff375963a98c0b3212a3349a52f39b5`.
The direction-labelled views establish that only the west/minimum-X plane
moves, by 1/8 per bite; the east and both Z planes remain fixed. Bite zero uses
the ordinary side on west and bites one through six use the inner crumb face.
Bounds are `[16+32*b,0,16]..[240,128,240]` in 1/256 coordinates, with
coordinate-derived UV cropping and no reflection or orientation variant.

Local evidence hashes:

- west: `90dfa79308d5efaddf918810dc2cdca4836148bc5910bf7631a054b2256f7a1b`
- east: `0b3a12dcdbaaf8205ea55341ea64413cd0d0beef4b6d835b627e437ed908bad`
- top: `61c70414014d178795318a60f31d12b28fd57066d20a2c3e04801bc5fc35135e`
- north: `585ef3522c9d7278fdf7d339012b0ce90c385e0a5a60770878e5ee8aa5b914ba`
- south: `d1c7e50a1eec3667db675faa03ea831ac3564bdf792eee0f6c6a2dc247f0f2bf`
- exact readbacks `b=0..6`:
  `966292f33f9601de846c2dd8acdb40fda00452798a132d25fa77cf72303aad53`,
  `ffd38d9e9800f8b0cd800bf6eb22b455e091a90ca409564f995a5cbbd8b50049`,
  `57c2ac1a0fbeab3fa6f5f277b782f3321ed15748e2e49b466e5db358ea03d571`,
  `31981261252253749900e676976759c52278eb3fd8d7e3dd8e07bb249807eee6`,
  `0b923a17fc8e217feb97735e242eb691148648e5de1113661d5aafdbc460db75`,
  `35086ab57276ba464d9ae177c17f0da81d2fac9fc418ec52aa4ab6f563b47637`,
  `0c8e224c9273fb7df92c1c7ae3c55d37d691010d8caf66a86845eb795957ade7`.

The deterministic registry SHA-256 is
`050cf1e79f9505cfcb240b1eb6627df95451e062e77b368b6d2700c21e68c3e6`.
The ignored pinned-pack MCBEAS blob used for the coverage gate has SHA-256
`e800994b4bb39e1afc3e77207b510998289b4be7684eb4ac38a0aea677931e94`.
The matching-view two-frame Cinnabar GPU presentation witness remains open.
