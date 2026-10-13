# Cinnabar API

The account core (`-cinnabar-api`) keeps one optional WebSocket to the Cinnabar API for release
notices and Cinnabar player badges. Without a signed-in account, or while the API is unreachable,
everything else works as before. The contract lives with the API in `bedrock-mc/cinnabar-api`;
`core/cinnabarapi/schema/v1.json` pins its schema.

**Sent:** the app version, update channel and visibility; a fresh Minecraft multiplayer token bound
to a per-connection key, with a signature proving that key; and batches of up to 128 XUIDs from the
current server's player list. Server addresses and names are never sent. Tokens, nonces and lookups
are never logged.

**Stored by the API:** only in memory, per live connection: XUID, visibility and channel. Releases
keep channel, version and publish time. Lookups are answered, not kept.

**Visibility:** Settings → General → **Show me as a Cinnabar player** (on by default) changes it
live. When off, other players see no badge for you; you still see theirs.
