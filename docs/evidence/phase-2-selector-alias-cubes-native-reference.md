# Phase 2 selector-alias opaque-cube evidence

Date: 2026-07-14

Protocol: 1001

Native version: Bedrock 1.26.33.1

No Mojang assets or screenshots are tracked by this manifest.

## Native local-only screenshot hashes

| Capture | SHA-256 |
|---|---|
| hay set/readback | `ca5fa40beaf3c550ae31901a227220f27041847b6cdaf18d5d0bf061a0d45760` |
| hay front | `b5fa80bac7f3ef5d3c2e7d52e8f7b5199de79a0e5309668af75b48dd0830210c` |
| hay top | `1bffe3e131072a302d6e0b0c6a5988a3cda4d99a81b6f1b1be2fee7907bc311c` |
| hay east | `900fa8475fc27184daa38454536ba482554c0beab369af6956940d732e73cd85` |
| bone front | `f59d30c90937db3cef6407a7847111781e7197e8f8215cbb5592612f96d8ab6f` |
| chiseled quartz front | `a58b5aa77fdaeab442153b7ac887575dc717ded7e2ce7579323a4b71bcf0c49e` |
| TNT false/true | `f51bff38d4f1284072a039dbf45f11a53c59e0777f796b23d12cabc0d5a310c2` |
| hay deprecated 1/2/3 | `0e49a4487323b5978b9500ca2eed8acca2b01880340d5f82b1d5189a2ac342ab` |

## Generated artifact hashes

| Artifact | SHA-256 |
|---|---|
| `block-registry-v1001.bin` | `9f67a14d73cf958b53557cc31c601168aa0eb95c5d46dfac1299f8412a0cb74f` |
| ignored `vanilla-v1001.mcbea` | `18a4718d6fd03a66c0eb30e0a28444dcf80159c658cf4f7712e5ff342f7740ca` |
| `visual-coverage-v1001.json` | `5380b1e9d3c191cb7dc22231bd17c4ed4b7cf9346c3f06b79ed8efa94670835a` |

## Reproduction commands

```powershell
go test ./tools/registrygen -run ReviewedSelectorAliasCube -count=1

go run ./tools/registrygen `
  -out crates/assets/data/block-registry-v1001.bin `
  -pmmp <PINNED_PMMP> `
  -prismarine <PINNED_PRISMARINE> `
  -valentine-palette crates/protocol/vendor/valentine/bedrock_versions/v1_26_30/src/block_palette.bin `
  -valentine-blocks crates/protocol/vendor/valentine/bedrock_versions/v1_26_30/src/blocks.rs

cargo run -p asset-compiler --bin assetc --locked -- compile `
  --pack <PINNED_VANILLA_PACK> `
  --registry crates/assets/data/block-registry-v1001.bin `
  --biome-registry crates/assets/data/biome-registry-v1001.bin `
  --out .local/selector-alias/vanilla-v1001.mcbea

cargo run -p visualcoverage --locked -- baseline `
  --registry crates/assets/data/block-registry-v1001.bin `
  --assets .local/selector-alias/vanilla-v1001.mcbea `
  --invisible-allowlist crates/assets/data/visual-invisible-v1001.json `
  --out crates/assets/data/visual-coverage-v1001.json

Get-FileHash crates/assets/data/block-registry-v1001.bin -Algorithm SHA256
Get-FileHash .local/selector-alias/vanilla-v1001.mcbea -Algorithm SHA256
Get-FileHash crates/assets/data/visual-coverage-v1001.json -Algorithm SHA256
```

## Automated result

- Registry generations: byte-identical.
- Full pinned-pack compilations: byte-identical.
- Diagnostic ratchet: `2506 -> 2479`.
- Removed diagnostic IDs: exactly 27; additions: zero.
- Matching-view two-frame Cinnabar GPU witness: open.
