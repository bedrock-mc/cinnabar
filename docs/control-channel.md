# Control channel

`-control-status` binds a local JSON-RPC 2.0 endpoint (4-byte big-endian length frames, one request per
connection). Every result carries `schema_version: 1`. Errors are sanitized: fixed messages, never paths or
upstream text. Rust clients: `crates/bridge`, re-exported by `protocol::launcher_control` and
`protocol::world_control`.

| Method | Params | Result |
| --- | --- | --- |
| `status.v1` | none | lifecycle, pack admission, optional `transfer` |
| `pack_application.v1` | `attempt_id`, `applied` | status |
| `world_*.v1` | see `docs/local-worlds.md` | worlds / status |
| `realms_list.v1` | none | `realms: [{name, state, target, address?}]` |
| `friends_list.v1` | none | `friends: [{gamertag, xuid, world_name, members, max_members, handle_id?, address?}]` |
| `connect.v1` | `kind` = `raknet` (`host:port`), `realm` (id), `friend` (xuid); `value` | empty; next client connection dials it |
| `account_status.v1` | none | `account` |
| `sign_out.v1` | none | `account`; deletes cached tokens |
| `events.v1` | none | `auth`, `disconnect?`, `transfer?`, `pack_download?` |
| `featured_servers.v1` | none | `servers: [{name, address, caption, description?, news_title?, news?, logo, screenshots, games}]` |
| `profile.v1` | none | `profile: {gamertag, xuid, gamerpic, real_name?, presence_text?, gamerscore, friends, followers}` |
| `home.v1` | none | `home: {messages, inbox, treatments, realm_invites, live_events, persona_head}` |
| `message_event.v1` | `event_type`, `instance_id?`, `report_id?`, `button_id?` | empty |
| `ping.v1` | `addresses` (at most 64) | `servers: [{address, online, players, max_players, ping_ms, motd?}]` |
| `store_home.v1` | `page?` (session-config `knownPages` name, default `home`) | `page: {id, rows: [{id?, title?, kind?, offers}], inventory_version?, truncated?}` |
| `store_search.v1` | `term`, `filter`, `order_by`, `count` (<=50), `continuation` | `offers`, `continuation?`, `truncated?` |
| `store_offer.v1` | `offer_id` | `offer`: offer plus `description`, `screenshot_urls`, `platforms` |
| `store_balance.v1` | none | `balances: [{currency, amount}]` |
| `store_entitlements.v1` | `offset?`, `limit?` (<=800), `refresh?` (first window: ask the service to refresh the inventory) | `owned` ids, `total`, `offset`; advance `offset` by `len(owned)` |
| `store_row_more.v1` | `continuation` (a row's `continuation`) | `offers`, `continuation?` |
| `store_purchase.v1` | `purchase_id`, `offer_id`, `store_id?`, `currency`, `amount`, `unit_duration_seconds?`, `confirmed` | `status`, `http_status`, `marketplace_error_code`, `correlation_id`, `replayed?` |

Artwork fields are `{url, path?}`: an HTTPS URL the client downloads and caches; `path` is set only
for persona art the core renders. Realms also
carry `owner`, `motd`, `world_type`, `online_players`, `max_players`, `days_left`, `expired`, `member`.
Feed sources: `docs/menus-services.md`.

`account` / `auth`: `state` is `offline | signed_out | awaiting_code | signed_in | failed`, with
`verification_uri` and `user_code` while awaiting a code, `gamertag` when signed in, `reason` on failure.
`disconnect`: `{reason, message, sequence}` and `transfer`: `{host, port, sequence}` stay until the next
connection attempt begins; poll and compare `sequence`. `pack_download`: `{received_bytes, total_bytes}` while the
newest attempt downloads the server's packs (chunk bytes against the admitted offer).

`offer`: `{id, title, creator?, content_type?, thumbnail_url?, store_id?, prices: [{currency, amount}], rating?, tags?, owned}`.
`store_purchase.v1` spends real Minecoins: it is rejected (`-32602`) unless `confirmed` is `true` and every field is well formed,
is sent to Mojang at most once per `purchase_id` (a replay returns the recorded result with `replayed`), and locks the offer
while in flight. `status` is `purchased | price_mismatch | precondition_failed | failed | unknown`; after `unknown` re-read
`store_balance.v1` and `store_entitlements.v1` before retrying (the offer stays locked for two minutes). Details in
`docs/marketplace-services.md`.

Errors: `-32031` purchase in progress, `-32032` `purchase_id` reused with other parameters, `-32033` unknown store page,
`-32020` not signed in, `-32021` service unavailable, `-32022` invalid target, `-32023` launcher services
disabled, `-32602` invalid params. Precedence for the next connection: pending server transfer, `connect.v1`
selection, open local world, `-upstream`. `world_open.v1` clears the selection and any pending transfer.

Limits: after `sign_out.v1` the running core stops using the account; signing in again needs the device-code
flow (`-auth-events`) and a core restart. Catalog calls can take tens of seconds; requests are served concurrently.
