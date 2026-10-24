# Private Dragonfly world connection

The original local-server process connects back to an authenticated numeric
loopback endpoint. Bootstrap capability bytes travel through its existing stdin
pipe. This connection carries no new player authentication authority. The Rust
accepting owner verifies Login, checks the original hosted membership and nonce,
and retains that approval through initialization and play. The signed client
object is preserved verbatim, including its unused fields.

`data/host-world.json` defines this process protocol; `tools/bridgegen` generates
the Rust and localserver Go declarations. Framing reuses `BridgeCodec`. The child
captures its final `Config.Resources` and serves bounded ranges of those exact
archives. World startup carries the actual Go-generated packets, rather than a
second JSON representation of `GameData`.

One packet and receipt is outstanding in each direction. The same reader keeps
handling receipts and close controls while its application payload is occupied.
Writes and partial reads remain with their original owner across canceled waits.
The application must keep actual protocol write futures pinned; canceling the
whole peer retires its sockets. Explicit closing uses one finite original
deadline and retains unresolved partial cleanup for retry while that owner lives.

The per-packet process receipt means actual external transport submission,
without promising remote Minecraft processing. Its throughput and join cost
remain unqualified. This checkpoint starts no native public server or account
worker and advertises no activity, invitation or supported route. Server
signaling, multiple concurrent application peers, end-to-end world acceptance
and performance require separate qualification. Go remains the default.
