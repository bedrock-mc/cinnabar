# Cinnabar API

The account core (`-cinnabar-api`) keeps one optional WebSocket for release notices and Cinnabar
player badges. Signed-out clients stay offline. An API outage does not stop gameplay or the
normal signed-manifest update checks. `core/cinnabarapi/schema/v1.json` pins the server contract;
new v1 fields are optional, and unknown fields and message types are ignored.

**Sent:** app version, signed-manifest build number, update channel and visibility; a Minecraft
multiplayer token bound to a proof key; and XUID batches from the current server's player list.
The socket connects before a token is minted. The key and token are reused until near expiry,
and each handshake signs a fresh challenge. Tokens, challenges and lookup contents are not logged.
Server addresses and names are not sent, but roster lookups can reveal which players share a server.

**Visibility:** Settings → General → **Show me as a Cinnabar player** is on by default and changes
live. Turning it off removes your visibility from the API immediately; other clients' badges can
remain until their cached answers expire. A badge means the account uses Cinnabar, rather than
proving its client or showing exact online status. The service retains visibility briefly after
a disconnect, as described in its Privacy documentation. You can still look up badges while hidden.

**Lookups:** the server advertises its batch limit, XUID budget, answer lifetime, keepalive interval
and reconnect cap. Older servers use the pinned v1 defaults. New joiners are queued promptly;
refreshes coalesce older answers and drain each round before requeuing refreshed players, so small
negotiated batches still reach the roster's tail. Positive badges expire even if a refresh fails.
Changing server sessions clears the cache and rejects older replies; departed XUIDs are pruned.

**Retries and updates:** HTTP 429 and close code 4429 impose a minimum retry delay, with additional
jitter. Server disconnects spread reconnects across 30 seconds. Repeated invalid-frame rejections
park the API connection until restart. Release pushes schedule a signed-manifest check within five
minutes; if the manifest still predates the announcement, a few delayed retries allow publication
caches to catch up. Release notices include the build and are sent only after publishing a changed,
signed manifest. Packaging rejects placeholder API origins.

**Shared emotes:** the client reports up to 128 nearby player identities and local custom
playback over the same authenticated connection. The API relays starts, phases and stops only
when both clients report each other nearby. The visibility option controls publishing.
Changing worlds clears watches and received playback. Unknown clips are skipped, and each
received dance expires after at most 15 seconds without renewal. Disconnects and visibility
opt-out stop playback immediately when delivered. Server addresses and names are not sent.
This requires the API's optional `emote_state`/`emote` extension; older API servers simply
ignore the updates. Local playback works while the API is unavailable.
