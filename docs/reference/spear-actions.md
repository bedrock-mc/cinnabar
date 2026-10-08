# Spear actions

Spear attachables use the selected resource pack's geometry, textures and
animation controllers. Player pose inputs complete the spear variables when
the player definition does not author them itself. The item must carry the
`minecraft:is_spear` tag; a name suffix or another kinetic weapon does not select
the vanilla spear pose.

Startup behavior data and session item components supply swing duration, attack
cooldown category and kinetic phase lengths. Session entries override the startup
catalog, and disconnect restores that catalog. Durations are normalized once to
simulation ticks. First-person and third-person clips receive jab progress,
charged-use phases and confirmed kinetic-hit timing from that same state.
Both flat authored components and wrapped session kinetic components retain
their phase timings. The default Java animation setting preserves the native
charged spear posture for explicitly tagged spear items.

An attack press sends the item-directed attack
transaction with the selected authoritative stack, aim point and cooldown flag.
It does not report an ordinary missed swing. A full outbound queue restores the
unadmitted cooldown and swing so the press can retry. A new session clears timers;
a position correction retains them. Piercing attacks take priority when the view
ray hits a block and suppress mining while the attack button stays held.
Damage and Lunge movement remain server-owned.

Attack priority follows the published
[piercing weapon component](https://learn.microsoft.com/en-us/minecraft/creator/reference/content/itemreference/examples/itemcomponents/minecraft_piercing_weapon?view=minecraft-bedrock-stable)
contract.

Regressions cover malformed optional components, authored versus throwing
cooldowns, category expiry, admission rollback, invalid aim, session reset,
animation queries, pack-authored overrides and actual compiled spear carriers.
Live before/after captures show the jab, first-person charged hold and raised
third-person arm. A server-confirmed Lunge jab moves the fixed client while
the baseline stays still, without movement input during the action.
An exact-version native comparison of all attack target cases and pose samples
remains incomplete.
