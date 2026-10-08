# Built-in Java-look chat

The owner chose Java chat geometry for the built-in look. This is a styling
exception, not a vanilla parity claim. `assets/java-hud/ui/chat_screen.json`
changes presentation only; the existing chat editor and input host still send,
paste, navigate sent history, and complete commands. The owner selected built-in
chat over server layouts. Its screen, suggestions and HUD history retain their
geometry and style; other server HUD widgets still use the pack stack.

The focused history ends at the same bottom offset as HUD chat. The input bar
and history share the pack's chat width. Focused history does not fade or give
way to autocomplete; suggestions overlay it immediately above the input.

Video settings expose **Chat Position** with **Bottom** (the existing default)
and **Top**. The saved preference moves HUD notifications and focused history.
Top notifications reserve the native position and days-played label heights.
The editor, suggestions, server HUD widgets and built-in chat priority retain
their existing behavior. This placement choice is a Cinnabar presentation
extension, not a new vanilla parity claim.

## Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Chat controller | Own clipboard paste and binding registration. Current binding implementation details remain unverified. |
| Sending | Send the message or dismiss blank input. |
| Sent history | Select previously sent messages. |

- Vanilla `v1.26.50.4/full/resource_pack/ui/chat_screen.json:308`: the
  `messages_factory` and scrolling panel, including jump-to-bottom on update.
  Lines 339 and 523 define the editor content binding and autocomplete rows;
  line 948 defines the chat screen and cancel mappings. `ui/ui_common.json:4427`
  defines the scroll view, and `ui/hud_screen.json:3574` defines HUD input policy.
  `texts/en_US.lang:584` supplies the existing chat title localization.

The offline snapshots composite the real carrier's UI over a plain backdrop.
They verify overlay geometry and HUD layering, not live world rendering or
target-platform visual parity.
