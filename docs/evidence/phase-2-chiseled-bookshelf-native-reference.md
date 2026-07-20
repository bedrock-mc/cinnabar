# Phase 2 chiseled-bookshelf native reference

Date: 2026-07-14

Native client/BDS release line: 1.26.33.1

Pinned resource pack: `bedrock-samples` tag `v1.26.30.32-preview`

The local BDS reference gallery used canonical block-state commands only:

```text
/setblock <x> <y> <z> minecraft:chiseled_bookshelf ["books_stored"=<mask>,"direction"=<direction>]
```

Direction was checked from the visible front of an asymmetric mask:

| direction | front |
|---:|---|
| 0 | south |
| 1 | west |
| 2 | north |
| 3 | east |

The accepted one-hot masks establish the static six-bit slot order:

| mask | visible occupied slot |
|---:|---|
| 1 | top-left |
| 2 | top-middle |
| 4 | top-right |
| 8 | bottom-left |
| 16 | bottom-middle |
| 32 | bottom-right |
| 63 | all six |

Mask 5 was also checked from every front and showed top-left plus top-right.
The six coplanar front rectangles tile one complete face as three columns by
two rows; shared position and UV boundaries are encoded once so adjacent slots
have neither gaps nor overlap.

Local-only native captures (not committed):

| File | SHA-256 |
|---|---|
| `%TEMP%\cinnabar-native-chiseled-bookshelf-onehot-20260714.png` | `9f04fccb99e83779440bd0e3093d2a89d54b12973c0c165b49d777fa0e0b0fb3` |
| `%TEMP%\cinnabar-native-chiseled-bookshelf-onehot-20260714-crop.png` | `b21e40d133fdce0abb6903578e952070b79de286116dfceb4672574fdb99d283` |

The screenshots are geometry/slot-order cross-checks. The pinned registry,
collision source, `blocks.json`, and terrain sources remain the authority for
state identity, full-face semantics, texture routing, and texture payloads.
No Mojang image or native screenshot is stored in the repository.
