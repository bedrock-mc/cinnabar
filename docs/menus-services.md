# Menu service feeds

Which services feed each out-of-game screen in the vanilla 26.30 client, and what Cinnabar
serves over the control channel (`docs/control-channel.md`). Endpoints and fields describe vanilla behavior and the open-source
gophertunnel/go-xsapi/go-playfab libraries.

Tokens: **MCToken** is the Minecraft-services `Authorization` header from the discovery `auth`
environment (`POST {auth}/api/v1.0/session/start`, started from a PlayFab session; its result also
carries `treatments`); **XSTS(rp)** is an Xbox Live token for relying party `rp`. MCToken calls
also send `Session-Id` and, for messaging, `Accept-Language`.

| Surface | Service call | Auth | Cinnabar |
| --- | --- | --- | --- |
| Discovery | `GET client.discovery.minecraft-services.net/api/v1.0/discovery/MinecraftPE/builds/<ver>` | none | `catalog` |
| Servers tab | `POST {layout}/api/v1.0/layout/ServerTab` body `{}`: fabs of experiences, each joined at connect by `POST {gatherings}/api/v2.0/join/experience` | MCToken | `featured_servers.v1` (every experience once) |
| Live events | none: the current client never requests gatherings `/api/v1.0/config/public` (the service answers 404), so the start-screen gathering panel stays hidden | — | not served |
| Messaging (tiles, inbox, modals, toasts) | `POST {messaging}/api/v1.0/session/refresh` `{sessionId, continuationToken}`; reports `POST .../messages/event` | MCToken | `home.v1` `messages`/`inbox`, `message_event.v1` |
| Treatments | MCToken session result `treatments[]` | PlayFab | `home.v1` `treatments` |
| Realms worlds | `GET bedrock.frontendlegacy.realms.minecraft-services.net/worlds` | XSTS(`https://pocket.realms.minecraft.net/`) | `realms_list.v1` |
| Realms invites | `GET .../invites/count/pending` (bare integer) | XSTS(realms) | `home.v1` `realm_invites` |
| Friend worlds | `sessiondirectory.xboxlive.com` activity handles | XSTS(`http://xboxlive.com`) | `friends_list.v1` |
| Profile | `peoplehub.xboxlive.com` (gamertag, gamerpic, gamerscore, presence), social friends/followers | XSTS(xboxlive) | `profile.v1` |
| Persona head | `GET {persona}/api/v1.0/profile/xuid/<xuid>/image/head` (image bytes) | MCToken | `home.v1` `persona_head` |
| Server rows | RakNet unconnected ping (players, max, round trip) | none | `ping.v1` |

Message placement follows each message's `surface`: `PlayButton`/`MarketplaceButton` (start
button art), `InboxMessage`, `LoginAnnouncement`, `MarketplaceAnnouncement`,
`ToastNotification`, `SystemWhisper`. Messages need `id`, `surface` and `template`.

Not served: persona appearance pieces for the paper doll (`{persona}/api/v1.0/appearance/*`,
piece JSON not decoded), gathering venue/eligibility, store layout pages, player-safety polling.

## Screen bindings each feed populates

- **Start screen** (`start.start_screen`): `#gamertag_label`, `#playername`,
  `#gamertag_pic_and_label_visible` (profile); `#gathering_*` button and countdown
  (gatherings); `#realms_notification_count` (invites, not served); Play/Store art
  (`#play_button_art_*`, `#store_button_art_*`) and `#unread_notification_icon` (messaging, not
  served). The home news carousel of newer clients is OreUI, not part of `ui/*.json`.
- **Play screen** (`play.play_screen`): `third_party_server_network_worlds` items
  (`#third_party_server_name`, `#third_party_server_message`,
  `#third_party_server_logo_texture_path`), the selected server's info panel
  (`#info_third_party_server_name`, `#description_label`, `#news_text`, `server_games_collection`,
  `server_screenshot_collection`); `personal_realms` / `friends_realms`
  (`#realms_world_player_count`, expiry); `friends_network_worlds` (`#network_world_header`,
  `#network_world_details`, `#network_world_player_count`); `servers_network_worlds` (saved).
- **Start screen** extras: messaging art on `#play_button_art_*`/`#store_button_art_*`
  (drawn by the gif renderer, first frame), banners, `#unread_notification_icon_visibility`,
  `#realms_notification_count`, and the `#gathering_*` live-event button.
- **OreUI routes** (bundle `data/gui/dist/hbui` of a local install; `routes.json`): profile
  (`/profile/:tab`, facet `vanilla.playerProfile`), play (`/play/:tab`, opt-in in 26.30),
  settings, inbox, friends drawer, disconnected, death and inventory. There is no OreUI home
  carousel in 26.30; the start screen is JSON-UI. Cinnabar draws the profile route natively.

Artwork is fetched by the core over HTTPS into a bounded per-run cache and drawn from local
files at up to 512 px on the full-resolution art pages.
