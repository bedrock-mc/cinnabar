# Absorption hearts

Vanilla rules for Bedrock 1.26.50:

| Input | HUD behaviour |
| --- | --- |
| `minecraft:absorption` | Read current points independently of its maximum; round positive fractions up to the next half-heart. Zero removes absorption. |
| HUD JSON-UI | `ui/hud_screen.json` places the native `heart_renderer`, gated by `#show_survival_ui`; there is no absorption binding or image control. |
| Placement | Append absorption after the maximum-health containers. Draw full hearts followed by an odd half-heart. Wrap every ten hearts with 8-pixel horizontal spacing and 10-pixel row spacing. |
| Poison and fatal poison | Health changes to poison sprites; absorption stays golden. |
| Wither | Absorption uses wither full and half sprites. Poison takes precedence when both effects are present. |
| Damage flash | All heart containers use `textures/ui/heart_blink` during the flash phase; absorption foreground keeps its ordinary full and half sprites. |
| Hardcore | Select the corresponding sprites under `textures/ui/hardcore/`. |

The HUD retains absorption as display points rather than an attribute maximum. Heart capture retains a fixed set of sprite candidates and generates cells only for rows intersecting the viewport or inherited clip. Cells fully outside the viewport or inherited clip do not create draw nodes. Invalid or unsupported current values are counted and skipped without ending the session.
