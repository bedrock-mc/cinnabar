# Cinnabar client parts for Dragonfly

`cinnabarext` offers signed client bundles and carries typed messages over the
existing Cinnabar wire v2 envelope. It has no dependency on the local server's
Experience runtime. The local server imports this same package.

The package lives in the existing Go module:

```go
import "github.com/cinnabar-mc/cinnabar/tools/localserver/cinnabarext"
```

Pin the module to the Cinnabar commit you deploy. Consumers can use upstream
Dragonfly v0.11.5 with upstream
Gophertunnel v1.62.0; this package needs only the public
[listener interface](https://github.com/df-mc/dragonfly/blob/v0.11.5/server/listener.go)
and session connection interface. The two-world example uses those dependencies
without replacing Dragonfly or Gophertunnel. The Cinnabar local server retains its
own fork pins for its other features. Older Dragonfly versions may need updating
to match the selected Gophertunnel protocol version.

## Setup

Create a stable signing seed with `cinnabar-cxb keygen server.seed`. Keep it outside
source control. Build client `.cxb` bundles with the existing SDK and bundler.
`ReadBundles` and `Config.BundleDirectory` validate archives with `cinnabar-cxb`;
install that executable beside the server, on PATH, or set `CINNABAR_CXB_BIN`.

```go
key, err := cinnabarext.ParseSeed(seedText)
// Handle err and retain a monotonically increasing deployment revision.
allowed := cinnabarext.Permissions(1 << cinnabarext.PermissionMessaging)
ext, err := cinnabarext.New(cinnabarext.Config{
    Key: key, Audience: "play.example.org:19132", Revision: revision,
    BundleDirectory: "bundles", AllowedCapabilities: &allowed,
})
// Handle err before configuring Dragonfly.
err = ext.WriteMarkerPack(userConfig.Resources.Folder)
// Handle err, then let Dragonfly read the optional marker resource pack.
conf, err := userConfig.Config(log)
// Handle err.
for i, listener := range conf.Listeners {
    conf.Listeners[i] = ext.Listener(listener)
}
```

The offer audience must exactly match the canonical host and port players join.
Do not require the marker pack. No carrier traffic is sent to a player until its
client initiates the signed handshake after discovery and consent.

Bundle delivery currently retains the local server's cache-only `cxb.invalid`
origin. Use `cinnabar-cxb seed-cache` on each test client before joining. This
package does not add a public bundle hosting service or change client execution
policy.

## Typed messages

A generated message implements `Route() Route` and `Encode() (Record, error)`.
`Route` contains the bundle, channel and schema revision from the shared message
declaration. Until generated Go bindings are available, a small application type
can implement this interface, as the example does. The signed manifest remains
the authority for direction, field types and bounds.

```go
ext.Send(p.UUID(), message)
count := ext.Broadcast(audienceUUIDs, message)
err := cinnabarext.OnMessage(ext, messageType, func(id uuid.UUID, message MyMessage) {
    // id comes from the authenticated session. Schedule any world work through
    // Dragonfly transactions; do not retain a *player.Player here.
})
```

Go does not support generic methods, so `OnMessage[T]` is a package function.
Register receivers before listening. A route has one typed receiver; duplicate
or undeclared routes fail registration. Callbacks run on the player's packet
reader and must return promptly. Send's boolean and Broadcast's count report
local admission, including schema checks, negotiated rate and size limits, live
grants and world fences. They do not acknowledge remote application processing.
`SendRecord` and `OnClientMessage` expose the lower-level record boundary used by
the local Experience adapter.

## World changes

Call `ext.WorldChanged(p.UUID(), worldID)` in the destination transaction after a
world transfer commits. A `WorldID` identifies a world instance, independently of
its dimension type. Call this for transfers between two overworld worlds too.
Repeated calls with the same nonempty ID are idempotent for that connection.

On wire v2 the server sends the existing `epoch` control as a request for a fresh
logical epoch. The client keeps its runtime, invokes the existing client-part
`epoch` callback and replies with `world_ack`, naming both the requested epoch and
its new logical epoch. A native `epoch` update cannot acknowledge a server notice.
Until the newest notice is acknowledged, sends are unavailable and old-world
inbound messages are discarded. Use `OnEpoch` to resend current state after the
fence opens.

The fence takes effect before the notice is written. Notices and their correlated
acknowledgments bypass the data rate budget, so exhausted data credits cannot delay
a transfer. A newer world supersedes the pending notice; only its acknowledgment
opens the fence. Superseded and repeated acknowledgments are ignored. There are no
retry timers. A write failure ends the optional client part while ordinary play
continues. `WorldChanged` returns false when there is no usable v2 session or the
world ID is empty.

Native dimension changes and explicit notices both advance the logical epoch.
Logical epochs are independent of the native dimension counter. The client never
resets its logical epoch to that counter on a later frame. Wire v1 remains usable
for its existing messages, but `WorldChanged` returns false for it. Clients that
predate server-originated epoch requests fall back from the optional extension
when a request arrives; their ordinary session continues. Existing bundle WIT
interfaces are unchanged. Server-notice acknowledgments require matching client
and helper revisions; the earlier plain `epoch` acknowledgment no longer opens a
server world fence. Existing native epoch updates and v1/v2 message records keep
their formats.

See [the two-world example](../../../examples/dragonfly-extension/README.md).
