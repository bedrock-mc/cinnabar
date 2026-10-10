# Discord integration

Discord presence is enabled by default using the built-in application; turn it off with
**Discord Rich Presence** in Video settings. To use another application, set
`CINNABAR_DISCORD_APPLICATION_ID` to its numeric Application ID before launching. No bot token or
client secret is needed. Setting the override to `0` disables presence entirely.

With the Discord desktop app running and activity sharing enabled, presence shows menus (also while
joining, so the in-world card is not held behind Discord's update rate limit),
`Playing on host:port`, `Singleplayer: <world>`, or a Realm, friend's world or experience without
its identifier, plus the original app icon served from GitHub. On a featured server the server's
own logo sits in the card's corner. The timer counts the current session in game and the launch
otherwise. Updates run over local IPC, reconnect automatically and follow Discord's rate limit. The
card never shows account details, Realm IDs or friend XUIDs.

While you play on a server, an experience, a friend's world or your own hosted world, Discord
friends can join you from your profile or a chat invite. The destination travels only in Discord's
join secret, and the joining client accepts only addresses it would itself publish. A friend's world
still needs the joiner to see it through Xbox, as in vanilla. Realms and Flat worlds are not
joinable. Cinnabar registers itself with Discord on each launch (`discord-<id>` in
`HKCU\Software\Classes` on Windows, a `.desktop` handler on Linux, Discord's `games` folder on
macOS) so an accepted invite starts the game when it is closed.

When a Discord user asks to join, a toast names them while you play and stays until you answer or
Discord closes the request, giving way to server toasts in between. Press **Open Notification**
(N by default, remappable in Controls), press the toast with a free cursor, or open the pause
screen to answer in vanilla's popup, which the launcher menus show directly. Accepting sends them
Discord's invite.

A Normal (dedicated-server) local world is hosted for Xbox friends while it is open, as vanilla
hosts worlds: it appears in friends' Friends tab (friends of friends may join, up to 8 players),
and the pause screen's friends button opens vanilla's Invite to Game screen. Each joiner must log
in with Xbox Live and present the one-time nonce the session issued them.
