# Server-confirmed player game modes

`SetPlayerGameType` is the client’s request and `UpdatePlayerGameType` is the server’s confirmation.
We identify it by our generated `McpePacketName::UpdatePlayerGameTypePacket`
enum, not a copied numeric ID. The generated packet preserves a game type, signed actor
unique ID and unsigned player-input tick.

## Local identity and default mode

The vanilla client matches the `UpdatePlayerGameType` target against player-list **unique IDs**. A
matching local player gets its mode changed and its UI publisher notified. A
matching remote player follows the remote-actor setter instead. A runtime ID, zero
or `-1` is not a wildcard target. The legacy `SetPlayerGameType` handler already has an implicit local target.

Raw game type `5` resolves through the level's default game type. Setting it
retains that raw default binding while using the effective mode for the
mode-change work. Cinnabar reuses its existing player/default-mode reducer:
an explicit player mode is independent of later default changes, whereas a player
bound to the world default follows those changes. Unknown well-formed game types
remain counted, ignored data, not a disconnect.

## Cinnabar receive path

The raw world-packet admission list and normalized event decoder now accept
`UpdatePlayerGameType`. Its targeted UI event retains the unique ID and tick until
the ordered world stream admits it against the bootstrap local unique ID. Only
then is it converted to the existing local game-mode event. The committed UI path
updates the retained HUD and existing movement/inventory capability inputs. A
targeted event injected directly into the UI is ignored because that layer cannot
establish local-player identity.

Regressions exercise generated packet encoding through raw ingress, all supported
mode/default mappings, signed identity and full-width tick preservation, malformed
truncation, foreign/sentinel targets, FIFO fencing, and committed HUD/input authority.
The October 1 offline vanilla-BDS run changes survival to creative and back
without reconnecting. macOS/Metal Retina captures `2026-10-01_23.37.28.png`
and `23.48.14.png` show the creative catalog and survival crafting inventory;
the intervening HUD captures show hearts/hunger returning in survival. The
server console confirms each mode command. This closes the missing-packet
functional regression, not historical replay or full vanilla visual parity.

## Creative destruction is mode-driven (2026-10-04)

Starting and continuing block destruction select the creative route from the
player's creative check alone. That check reads only the game type: Creative, or world-default
resolving to Creative. It does not inspect the Instabuild ability.
Destroy progress passes **Flying** into the destroy context, and the progress
and speed calculations apply the speed, hardness, harvest and movement
penalties; neither selects instant destruction from Instabuild. Flying and
Instabuild are independent abilities.

Cinnabar previously let received Instabuild override `instant_break`, so survival
could take the creative mining route after a mode change despite the correct HUD.
The capability now follows the confirmed game mode only. Regressions cover
Survival with Instabuild enabled, Creative with it disabled, and a committed
Creative → Survival → Creative transition while retaining the same wire evidence.
Other ability grants and the passive evidence owner are unchanged. Vanilla mode-layer
refresh is a separate incomplete gate; clearing all received layers
would incorrectly discard higher-priority server grants.

In the October 4 macOS/Metal scratch-BDS run, the console confirmed the local
player's Creative-to-Survival transition. The user tested the rebuilt client and
confirmed that breaking and dropped-item testing now work correctly. The
capability matrix and retained-evidence transition regressions also pass.

## Incomplete historical replay

The vanilla client applies a tick-zero `UpdatePlayerGameType` immediately. With a nonzero tick and
an eligible replay timeline, it instead inserts a game-type replay entry at that tick and
suppresses the immediate setter; otherwise it applies immediately. Cinnabar now
decodes the tick, but applies accepted updates at receive FIFO commit rather than
replaying historical movement authority. That timing/replay branch remains a
separate incomplete parity gate; fixing the missing confirmation packet does not
close it.
