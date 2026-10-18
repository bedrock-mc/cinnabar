# Browser relay

The Go relay keeps Microsoft accounts in memory and joins public RakNet servers
through the existing core session handler. WebTransport over HTTP/3 is the preferred
transport. HTTPS WebSocket connections provide control and play fallback channels.
The relay does not serve the browser page or runtime assets.

Build `core/cmd/bedrock-web-relay` with the core's Go toolchain and run the resulting
binary with operator configuration, for example:

```sh
bedrock-web-relay \
  -hostname relay.example.com \
  -udp :443 -tcp :443 \
  -tls-cert /etc/relay/fullchain.pem \
  -tls-key /etc/relay/privkey.pem \
  -origins https://play.example.com
```

Supply a CA-issued certificate whose names include the relay hostname. Keep its
private key outside the checkout. The relay checks those same paths every minute,
validates the hostname and validity period, then publishes the new pair to both
listeners. Replace the certificate and key together (for example, switch a symlink
to a completed renewal directory). A missing, mismatched, malformed or expired
replacement logs a warning and retains the last good pair. Existing connections
and in-memory accounts remain active. Renew before expiry: retaining an expired
old certificate cannot make new handshakes trusted. A process restart still loses
accounts. For development, use a local CA trusted by the browser; there is no
certificate-validation bypass. Host the page in a secure context. An empty origins
list accepts pages from the relay host; otherwise supply exact page origins,
including scheme and any nondefault port. Forwarded address headers are ignored.

Open the configured **UDP** port end to end for HTTP/3, and the configured **TCP**
port for HTTPS WebSocket fallback. They may use the same port number. Permit
outbound UDP to public game servers and HTTP/HTTPS for resource packs and Microsoft
sign-in. An HTTPS reverse proxy must explicitly support WebTransport Extended
CONNECT and UDP; ordinary HTTP forwarding does not provide that path. Static
hosting can run separately.

The shared [bridge contract](../crates/bridge/data/contract.json) owns endpoint
paths, the WebSocket subprotocol, Hello and control framing, message names, limits,
and conformance fixtures. Its generator supplies the same constants to Go and
Rust. WebTransport opens control first and play second on one connection. WebSocket
uses one connection per channel, with binary application framing. Both adapters
require Hello before account or gameplay bytes. Resume bearers are sent only inside
that protected framing, never in URLs, headers, subprotocols or logs.

A successful control Hello returns a rotated, short-lived bearer and a play
generation. Keep the replacement from each keepalive and answer with
`{"op":"pong","resume":"<received bearer>"}`. The relay repeats an unanswered
replacement with its remaining expiry and retains the acknowledged bearer at its
original expiry. A delayed pong cannot acknowledge an unseen replacement.
A resume retains the bearer presented by the browser until its replacement is
acknowledged.
Resumption can retain an account across network changes; the new peer address must
still satisfy admission. It starts a fresh play generation. Closing control cancels
play; resumed play waits for the cancelled handler to finish before restarting.
A refused resume requires an explicit fresh session; gameplay is never
replayed. Sign-out erases the account after play has ended.

Admission is limited to 256 retained accounts and 256 transport connections, with
four of each per peer IP. TCP sockets count before TLS and HTTP processing, and
QUIC connections count before their handshake. The TCP fallback accepts HTTP/1
only. Each QUIC connection allows one WebTransport session and must complete a
valid control Hello within the initial deadline, even when QUIC PINGs keep it active.
Upgraded connections retain their slot until the connection and handlers have ended.
Idle accounts expire after ten minutes. Action and upload rate limits, bounded
control messages, and deadlines apply to both transports. Control message bursts
are refused; only a pong confirming the current bearer extends the read deadline.
Public-address checks apply after DNS resolution to every game connection,
transfer hop, pack URL and
pack redirect; environment proxies cannot bypass them.

All current gameplay batches and pack chunks use reliable ordered play framing.
Inbound frame buffers grow with received data instead of allocating the full
advertised payload from its header. The shared bridge frame limit still applies.
WebSocket messages split larger bridge frames at the shared relay message limit;
message boundaries do not change the bridge byte stream.
Separate pack streams, gameplay datagrams and deployment/browser acceptance remain future integration work.

## Browser entry preview

From the workspace root, install the Rust `wasm32-unknown-unknown` target and Node.js,
then build a fresh static directory:

```sh
rustup target add wasm32-unknown-unknown
cargo run -p devtool --locked -- check-web
cargo run -p devtool --locked -- build-web \
  --assets .local/assets/compiled --out target/browser-site --release
```

`build-web` uses the exact `wasm-bindgen` version in `Cargo.lock`. It reuses a
matching CLI or installs it under this worktree's `target/web-tools`, builds wasm,
and stages the page, generated bindings, startup carriers and `assets/manifest.json`.
Carrier names, sizes and hashes come from the asset owner. The command refuses to
overwrite an existing output directory and publishes only after every step succeeds.
Game assets remain local runtime inputs: do not commit the bundle or redistribute
them without permission.

Serve the output through an HTTPS static server with JavaScript and wasm MIME types
(`text/javascript` and `application/wasm`). Enter its HTTPS `assets/` URL on the page.
Allow the page origin through CORS when assets or the relay use a different origin.
The relay serves no static files. The CI browser job runs the same `check-web`
command, including both JavaScript suites and the actual wasm target checks.

Enter an HTTPS asset directory and choose **Start client**. Startup validates the required
carriers and opens the shared game renderer on the canvas. Connect the relay, sign in if the
server requires an account, enter a server address, and choose **Join server**. Click the
canvas for gameplay input; Escape releases the mouse. **Disconnect** retires the active
world, and server transfers start a fresh play session through the same relay controls.
The preview has a gameplay HUD. An optional sound bank streams exact HTTP ranges into the
shared decoder and mixer. Click the canvas to enable its Web Audio context. Missing audio
leaves the client muted and shows a status message. Interactive inventory, chat and form
controls, persistent pack import, and the Dressing Room remain incomplete.

The sound-bank endpoint must support `Range` and `If-Range`, return HTTP 206 with exact
`Content-Range` values and a stable strong `ETag`, and serve identity-encoded bytes. For a
separate asset origin, allow those request headers and expose `Content-Range` and `ETag`
through CORS. Audio requests omit credentials. Pack replacement and page disposal cancel
outstanding reads.

The menu integration exposes relay sign-in, cancellation, sign-out and join progress to
the game's existing screens. Addressed servers get the default Bedrock port when omitted;
Realms, friends' worlds and featured experiences are unavailable in browsers. Its browser
preference document stores at most 32 addressed servers and the GUI scale modifier, with
a 32 KiB limit and visible read/write failures. It stores no account credentials. The
click and keyboard link bridge validates HTTPS links and owns its listener teardown.
Wiring these adapters into the canvas remains incomplete; the preview does not yet render
the game's menu or persist those menu preferences.

Back/forward-cache navigation suspends input and audio while retaining the page's wasm
objects and canvas listeners. Returning reopens relay control with any unexpired resume
capability; sign in again if it expired. Audio resumes on the next canvas click. A final
navigation frees the page, input and listeners and closes audio.

Server resource packs use one server-initiated reliable WebTransport stream per join. The play
Hello supplies a fresh nonzero `join`; the handoff's `pack_stream` names that join and declares
each archive's size and SHA-256. The stream begins with the join as a big-endian u64, followed
by ordinary length-framed PackData messages with their handoff indices. FIN completes the
archive barrier. The receiver admits the whole selection before reading archive bytes and
rejects incomplete, corrupt, extra or misassociated data. Cancellation resets the stream.

WebSocket sends verified archives serially on the play connection. This is a fallback transport
limitation: bulk bytes can delay later play bytes on that TCP connection. Control cancellation
still has its own connection. A third socket would need another authenticated lifecycle and
admission path; the fallback keeps the existing two connections.

## Browser support and known limits

Use a secure context with WebAssembly, fetch and readable/writable streams. Transport
selection attempts a reliable WebTransport control stream and Hello before choosing
it; a construction, stream-opening or handshake transport failure can select the
bounded WebSocket fallback. An application refusal never triggers fallback. A
refused or expired resume requires an explicit fresh session. Browser networking or
enterprise policy can block either transport even when its constructor exists.

Current Chrome, Edge, Firefox and Safari are acceptance candidates, not certified
release targets. No browser-name whitelist decides support. Gameplay will also
need WebGPU and the completed shared client composition. This preview's account
controls and asset validation do not demonstrate rendering or successful gameplay.

A release still needs a synthetic valid upstream join covering Connect, handoff,
packs, play, transfer and fresh reconnect, followed by the real-browser matrix:
UDP blocked, stalled stream creation, background tabs, navigation and network changes.
That work follows gameplay composition. Memory peaks, frame pacing, audio and
WebGPU/Metal output comparisons remain unverified. The ordinary queue byte budget,
WebSocket message limit and upload burst are separate policies; none is a total
session or relay memory bound. Local worlds, Realms/friends/featured browsing,
native updating and persistent imported packs remain unavailable in this preview.
