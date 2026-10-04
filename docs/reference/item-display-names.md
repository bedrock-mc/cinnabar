# Stack display names

The selected-item HUD label and inventory tooltip resolve the same stack name:
an authoritative stack-response name, then retained NBT `display.Name`, then
the item's localized default. Inventory content and slot packets can carry
custom names without any stack-response correction. Those names must reach
the HUD directly from the presented stack, including during inventory prediction.

## Vanilla rules

| Rule | Behavior |
| --- | --- |
| Custom-name presence | Determined by the `display.Name` tag; an empty string still counts as a custom name. |
| Custom-name text | Reads `Name`, or the optional `FilteredName` alternative, without stripping format codes. |
| Precedence | Custom names override the localized default; the hover name ends with `§r`. |
| Formatting | Custom names receive `§o` before the item's own formatting and literal name. |
| Selected-item popup | Uses the same hover-name producer as the inventory tooltip. |
| Popup refresh | Refreshes when the selected slot or source changes, even between identical item kinds. |
| Localization | HUD item text is created with localization disabled; names arrive already resolved. |

## Cinnabar correction

HUD capture previously consulted only stack-response name overlays before
falling back to the localized identifier. The tooltip already read retained
NBT. Both paths now share the name-line resolver, preserving custom text and
its format codes, including explicit resets that override native italics or
component colors. A present empty name stays empty. Unreadable display data
falls back without ending the session. Item-text factory creation also disables
localization, matching the native controller's handling of already-resolved names.
Filtered-name selection remains outside
this correction; it retains the existing unfiltered presentation policy.

The selected-item timer also includes the hotbar slot. It no longer suppresses
the popup when two differently named swords have identical item IDs and metadata.
This does not introduce an inferred same-slot rename timer rule.

Synthetic regressions exercise inventory content through the ledger and real
HUD capture, compare HUD and tooltip names, check slot-change retriggering and
stable-frame timing, and cover empty names, localization, Unicode, unreadable
display data, response precedence and server-authored formatting.

## Validation

On 2026-10-04, the rebuilt macOS client ran on Zeno, whose status response
matched the pinned client game and protocol versions. The user exercised the
custom-name behavior in that client and confirmed that the fix worked.

The three focused selected-item-name regressions passed. Affected-crate
formatting, the architecture gate and compilation also passed. The wider
local test run was stopped at the user's request; Clippy was skipped. Full
local verification is therefore incomplete, and the PR still requires green CI.
