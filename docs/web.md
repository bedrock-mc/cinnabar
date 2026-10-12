# Browser build

The client compiled to WebAssembly, rendering through WebGPU. Browsers cannot send UDP or run the Go
core, so a web relay (`core/cmd/bedrock-web-relay`) runs both for them. Remaining gaps are tracked in
`plan.md` under the browser build.

## How it fits together

| Piece | Where | Role |
| --- | --- | --- |
| Page | `web/site` | Checks for WebGPU, asks for Minecraft EULA consent, downloads the game files, starts the client. |
| Client | `web/client` (`web-client`) | The `app` crate compiled to `wasm32-unknown-unknown`, started with `CoreLink::Relay`: it signs in and joins from its own menu. |
| Relay | `core/webrelay` | One in-memory session per browser: Microsoft sign-in and Bedrock joins. |

The relay serves three endpoints, all on the page's own origin:

- `/v1/control` creates or resumes a session and runs device-code sign-in. The session ID is the
  browser's only credential and travels as a WebSocket subprotocol (`sid.<id>`), never in a URL. Tokens
  stay in memory and are erased on sign-out or after ten idle minutes.
- `/v1/session` carries the core's session protocol, byte for byte as on `session.sock`.
- `/v1/vanilla-pack` streams the pinned sample pack from `assets/vanilla-source.json` without storing
  it, since browsers cannot fetch GitHub release assets cross-origin.

Sessions join addressed RakNet servers only. Every upstream socket, including pre-login transfer hops
and the HTTP connections of URL pack downloads and their redirects, dials through a dialer that
refuses non-public addresses after name resolution. Origins, sessions per
address, actions per minute and browser upload rate are limited.

## Settings

Browsers start without multisampling or clouds and at the smallest render distance, since a tab
shares the GPU with the page and the browser cannot report installed memory
(`launcher::menu::settings_options::platform_defaults`). Saved choices still win, and resetting the
Video section returns to these browser defaults.

## Running it locally

```sh
make assets physics-assets      # the compiled carriers the relay serves
make web web-relay              # WEB_PROFILE=play builds faster than release
target/release/bedrock-web-relay -listen 127.0.0.1:8787 -site target/web-site \
    -dev-assets .local/assets/compiled \
    -dev-allow-private-upstream # only for joining a server on this machine
```

Open `http://localhost:8787/`, agree to the Minecraft EULA, and add a server such as the local
Dragonfly server (`make local-server`, `127.0.0.1:19132`) on the Play screen. The page starts the
client only when the relay serves game files, since browsers do not compile the pinned pack
themselves. `-trust-forwarded-for` is refused unless the relay listens on loopback;
`-dev-allow-private-upstream` on another address warns, since every client can then reach the
relay's private networks.

To test from another device on your network, browsers need a secure page for WebGPU and the device
needs no game files of its own:

```sh
target/release/bedrock-web-relay -listen 0.0.0.0:8787 -dev-self-signed \
    -dev-assets .local/assets/compiled -dev-allow-private-upstream -site target/web-site
```

`-dev-self-signed` serves HTTPS with a generated certificate the browser asks you to accept once.
`-dev-assets` serves your Mojang-derived compiled carriers to anyone who reaches the relay, so keep it
to private networks. `make web-dev` runs this setup with a local Dragonfly server and keeps one
certificate in `.local/web-dev`, so each device accepts it only once.

## Downloads

The page asks for Minecraft EULA consent before anything else, as the desktop's first run asks
before it fetches the vanilla pack: Agree downloads the game files and starts the client, and
Decline downloads nothing. The page remembers the consent with its text and link, so changing either
asks again. A returning player still clicks Play, since browsers allow sound and mouse capture only
after a click.

The relay serves the site and development carriers compressed (`core/webrelay/static.go`): zstd
where the browser accepts it, else gzip, each file compressed once and cached until it changes, and
sent stored when compressing saves too little. Release builds also ship precompressed `.zst` and
`.gz` copies of the client module, which the relay sends as they are. `Range` requests, as for the
sound bank, always get stored bytes. Every file carries an ETag and `Cache-Control: no-cache`, so a
rebuilt site never runs stale. Compressed responses name their decoded size in `X-Decoded-Length`,
which the page's progress counts against.

The page keeps the client module and each downloaded carrier in the `cinnabar-assets` Cache Storage
cache with its ETag, and on the next Play asks the relay with `If-None-Match`; an unchanged file
answers 304 and loads from the cache, so a repeat Play downloads only what changed.

## Accounts, servers and settings

The client opens the relay's control socket itself (`bridge::RelayControl`) and keeps its session ID
in the tab's session storage, so a reload resumes the same signed-in session. The menu signs in
through it (`launcher_host::relay_account`): the device code shows in game, and the sign-in
prompt's link button opens Microsoft's page inside the click itself, since browsers block a tab
opened afterwards. The Play screen's servers join through the relay with the default port added
where an address names none; Realms, friends' worlds and featured experiences are not joinable.

Settings, video and graphics choices and saved servers persist in the page's local storage under
`cinnabar.file:<path>` keys (`launcher::browser_files`), where the desktop keeps the same files
under its config root.

## Sounds

The page loads carriers into memory before the client starts, except a sound bank the relay serves.
For that bank it reads only the header and index; the client downloads each sound with a `Range`
request the first time it plays, so that play starts late. Downloaded sounds stay in the
`cinnabar-sounds` Cache Storage cache, keyed by a hash of the bank's index, so later visits play them
at once and a recompiled bank never reuses old bytes. The index itself stays in
`cinnabar-sound-index` under the hash that ends it; each visit reads only that hash to check it.

## Resource packs

Settings → Global Resources works as on desktop: Import opens the browser's file picker for
`.mcpack`, `.mcaddon` and `.zip` files, and a pack file dropped on the game imports too. The page keeps
imported packs in the `cinnabar-resource-packs` IndexedDB database (`web/site/packs.js`), so they
survive reloads. The client runs the desktop pack library over that store: the catalog and icons
load at startup, and an archive loads into memory only while the library reads it, as when the
active stack is applied. Applied packs layer below a server's packs as on desktop.

## Deploying

Run the relay on loopback behind a TLS reverse proxy that forwards WebSockets, for example Caddy:

```caddy
play.example.com {
    reverse_proxy 127.0.0.1:8787
}
```

with `bedrock-web-relay -listen 127.0.0.1:8787 -site <site> -trust-forwarded-for`. Pages served from
another origin also need `-allowed-origin <host>`.
