# Phase 2 FlowerBed native reference

## Evidence boundary

This measurement was made on Microsoft Minecraft for Windows `1.26.3301.0`
against BDS `1.26.33.1`. The pinned asset input is
`bedrock-samples` tag `v1.26.30.32-preview`, archive SHA-256
`12d5cddc03acd507e9e0bd412f2e94d34d0a1a855758af7a9eef61b03630ad7c`.
Those builds are from the same release line, but they are not the exact pinned
preview build. This is compatibility evidence for state semantics; it is not
an exact pinned-client pixel-parity claim.

The native options file recorded `gfx_field_of_view=90.8`,
`gfx_field_of_view_toggle=1`, `graphics_mode=2`, `gfx_smoothlighting=1`,
`gfx_fancyskies=1`, `gfx_msaa=2`, and `gfx_upscaling=1`.

## Fixture identity

- State-set SHA-256: `a2fe82092cb22835a0553091ecfcdd67cedcddc9e791feb2d0ddeff9fe091f15`
- Relative-layout SHA-256: `e6eb62b75661d8de7508bbb40095e105301051d22462ef39f82f4226528ef763`
- Elevated native gallery command SHA-256: `95e6fe3c673cec9dbc92dbf0c17ef2bc37c1f2dc7359aaef01d157749105e136`
- Diagnostic-pack evidence-manifest SHA-256: `1b8e60d6b75413848484fa9064295978aca19affff6abd9f740a29626ebcf5b0`
- Diagnostic source-identity SHA-256: `2da14a78c0dda9dcb2e9794e0a2f73555f06239340d4e04cb8c3fe9a4693d8cb`
- Gallery origin: `(86,152,190)`; growth advances by `+4 X`; direction
  advances by `+3 Z`; `pink_petals` occupies Z `190..199` and
  `wildflowers` Z `202..211`.
- Adjacent calibration cubes were placed at flower X `+1`. After cube-bearing
  calibration captures, they were replaced with air to expose every coloured
  flower and stem plane in the overhead mask capture. The resulting dirt
  squares preserve the positive-X reference in the image.

The generated acceptance fixture defines the fixed elevated camera commands:

```text
tp @a[name=PlayLunarMC] 100 182 200 facing 100 152 200
tp @a[name=PlayLunarMC] 100 160 156 facing 100 152 200
tp @a[name=PlayLunarMC] 144 160 200 facing 100 152 200
tp @a[name=PlayLunarMC] 62 178 162 facing 100 152 200
tp @a[name=PlayLunarMC] 138 178 238 facing 100 152 200
```

The selected generated terrain obstructed some external fixed positions (the
fixed north position was underwater). The mapping was therefore adjudicated
with the unobstructed overhead pose and these additional calibrated close
poses: `(100,164,186)`, `(118,164,200)`, `(82,170,186)`, and
`(118,170,214)`, all facing `(100,152,200)`. Task 5 must still capture the
fixed positions in a terrain-safe fixture before claiming native/Cinnabar
pixel parity.

## Native screenshot evidence

All images were captured with native Windows GDI `CopyFromScreen`, written to
`%TEMP%`, inspected fresh, and left outside the repository.

| View | Temporary filename | SHA-256 |
| --- | --- | --- |
| Clean overhead | `cinnabar-flowerbed-native-clean-top.png` | `3e6c47b9b44b1b517a584459849d8ba144167c82a02367b211172f84d384aaa9` |
| Clean north close | `cinnabar-flowerbed-native-clean-north.png` | `3d1f82e43939e0c4e668a447a7c95ed289cad61ddce26b8bda8896950f0b27ef` |
| Clean east close | `cinnabar-flowerbed-native-clean-east.png` | `dbbcda7ab6828c015eb02132bd72d0b3e5d59c7e73eaab5da906264bb20b2199` |
| Clean north-west oblique | `cinnabar-flowerbed-native-clean-oblique-nw.png` | `e9d507f140a6426f1137629d1a31f8bf5df0039ba8707e1679bc54408ae2c1cb` |
| Clean south-east oblique | `cinnabar-flowerbed-native-clean-oblique-se.png` | `fc44b9b18cbcc5569cdcb644446d8635eb803e5d0a346138b32af32101713f8e` |

## Measured growth mapping

The unique quadrant colours make the active patch count directly observable
in the overhead image. The dirt square at X `+1` proves that screen-right to
screen-left is growth `0` through `7`. Both blocks and all four directions
show the same sequence:

| Bedrock growth | Active additive patches | Compiled layout |
| ---: | ---: | ---: |
| 0 | 1 | 0 |
| 1 | 2 | 1 |
| 2 | 3 | 2 |
| 3 | 4 | 3 |
| 4 | 4 | 3 |
| 5 | 4 | 3 |
| 6 | 4 | 3 |
| 7 | 4 | 3 |

Thus the explicit native layout mapping is
`[0, 1, 2, 3, 3, 3, 3, 3]`. Growth 4 through 7 do not wrap to growth 0
through 3 and do not add new geometry; each aliases the full four-patch growth
3 template for the same block and cardinal direction.

