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
private key outside the checkout. Restart the relay after certificate renewal;
accounts are lost on restart. Host the page in a secure context. An empty origins
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
Separate pack streams, gameplay datagrams, the browser adapter and entry page,
and deployment/browser acceptance remain future integration work.
