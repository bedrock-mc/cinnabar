# World item-drop audio

The target and pack are selected by `assets/bedrock-target.json` and
`assets/vanilla-source.json`.

## Vanilla rules

| Gesture | Feedback |
| --- | --- |
| Successful in-world single-item drop | One local `drop.slot` event at the player's attachment position |
| Successful in-world whole-stack drop | One event, independent of the item count |
| Failed drop, including an empty selected slot | No event |
| Inventory-screen drop | Does not use the world-input drop cue |
| Server reply or inventory correction | Does not repeat the local cue |

The pinned pack's `sounds.json` resolves `drop.slot` to `random.pop`, volume
0.30 and pitch 0.55–0.75. This is distinct from the pickup `pop` route. Playback
reads the active pack's route and sound definition, preserving overrides,
explicit silence and the player sound category.

The inventory ledger records admitted world gestures once. Presentation drains
them after world input in the same update, using the player's eye position rather
than the detached camera. Missing audio or world state discards the feedback
instead of replaying it later. Session changes clear pending feedback.

Regressions cover single/stack counts, failed gestures, screen drops, session
reset, transport and rejection, pack overrides and unavailable playback. An
optional local carrier fixture checks decoded PCM and mixer output; it names
missing fixtures when unavailable. This correction does not close the broader
audio parity gate or establish matched-version live acceptance.
