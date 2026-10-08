# Effective movement speed and sprint authority

Player speed reads the current value of the `minecraft:movement` attribute. An
attribute update replaces the previous modifier set with the packet's modifiers and
adjusts the final current to the packet's current. The wire value already includes
any sprint or other movement-speed effects. It is not a base walking speed.

Setting sprint, on the local player and through the sprint trigger, returns on
an unchanged actor flag. A mob adds or removes only its identified sprint
modifier when that modifier is absent or present. The sprint modifier has UUID
`D208FC00-42AA-4AAD-9276-D5446530DE43`, operation 2, operand 2; its factor is the
shared simulator sprint multiplier. Server metadata adopts the flag without
installing a local modifier.

Cinnabar now retains effective current and the identified packet modifier factor.
An incoming attribute replaces any locally predicted modifier, including when its
modifier list is empty. Local sprint edges add or remove only an installed sprint
modifier; repeated sprint requests cannot boost an effective server value again.
The simulator's pre-sprint input is adapted using its shared multiplier exactly
once. Custom Speed values are never inferred from the attribute's default.
Retained attribute corrections replay actual subsequent sprint transitions, and
authoritative flag rewrites preserve current rather than inventing a local edge.

The Zeno witness had current `0.1` when walking, `0.13` when sprinting, and empty
modifier lists for both. Before this correction it moved at 4.3173 and 7.2959
blocks/second respectively, applying the sprint factor twice. The regression
checks ground movement at approximately 4.3173 and 5.6125 blocks/second, preserves
custom `0.12` movement, and covers packet replacement, modifier removal, metadata
adoption, session reset, and delayed attribute replay.

Live confirmation on Windows, optimized `play` build `fd480811`, joining
`zenomc.org:19132` on 2026-10-04, measured median ground sprint speed 5.61233
blocks/second across 102 samples and walk speed 4.31717 across 43 samples.
The fixed clock measured 20.00175 ticks/second over 26.2977 seconds. Incoming
movement current still transitioned between `0.1` and `0.13` with empty modifier
lists; the user also confirmed that sprinting felt normal. This closes the
reported Zeno ground sprint speed gate.
