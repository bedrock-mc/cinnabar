package extension

import (
	"context"
	"errors"
	"net"
	"reflect"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/experience"
)

// testEpoch is the world epoch of the test clients' Ready.
const testEpoch = 3

// fakeConn is the connection that Dragonfly's listener would accept: it reads queued packets and
// records what is written to it.
type fakeConn struct {
	identity login.IdentityData
	mu       sync.Mutex
	in       []packet.Packet
	out      []packet.Packet
	closed   bool
}

func (c *fakeConn) queue(pks ...packet.Packet) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.in = append(c.in, pks...)
}

func (c *fakeConn) ReadPacket() (packet.Packet, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if len(c.in) == 0 {
		return nil, errors.New("no packet queued")
	}
	pk := c.in[0]
	c.in = c.in[1:]
	return pk, nil
}

func (c *fakeConn) WritePacket(pk packet.Packet) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.closed {
		return errors.New("closed")
	}
	c.out = append(c.out, pk)
	return nil
}

func (c *fakeConn) written() []packet.Packet {
	c.mu.Lock()
	defer c.mu.Unlock()
	return slices.Clone(c.out)
}

func (c *fakeConn) Close() error {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.closed = true
	return nil
}

func (c *fakeConn) IdentityData() login.IdentityData { return c.identity }
func (c *fakeConn) ClientData() login.ClientData     { return login.ClientData{} }
func (c *fakeConn) ClientCacheEnabled() bool         { return false }
func (c *fakeConn) ChunkRadius() int                 { return 8 }
func (c *fakeConn) Latency() time.Duration           { return 0 }
func (c *fakeConn) Flush() error                     { return nil }
func (c *fakeConn) RemoteAddr() net.Addr             { return &net.UDPAddr{} }
func (c *fakeConn) StartGameContext(context.Context, minecraft.GameData) error {
	return nil
}

// fakeListener is Dragonfly's default listener: it accepts its conns once each, and records the
// conns that it is asked to disconnect, which must be the conns it accepted.
type fakeListener struct {
	conns        []session.Conn
	disconnected []session.Conn
	closed       bool
}

func (l *fakeListener) Accept() (session.Conn, error) {
	if len(l.conns) == 0 {
		return nil, errors.New("listener closed")
	}
	c := l.conns[0]
	l.conns = l.conns[1:]
	return c, nil
}

func (l *fakeListener) Disconnect(conn session.Conn, _ string) error {
	l.disconnected = append(l.disconnected, conn)
	return nil
}

func (l *fakeListener) Close() error {
	l.closed = true
	return nil
}

// clientMessage is one message the Server passed on, with its payload described by value.
type clientMessage struct {
	player       uuid.UUID
	exp, channel string
	schema       uint16
	payload      string
}

// connFixture is one player's connection, accepted through the wrapped listener of a test server.
type connFixture struct {
	t        *testing.T
	s        *Server
	clock    *testClock
	listener *fakeListener
	inner    *fakeConn
	conn     session.Conn
	player   uuid.UUID
	manifest Manifest

	mu       sync.Mutex
	received []clientMessage
}

func newConn(t *testing.T, change func(*Config)) *connFixture {
	t.Helper()
	s, clock := testServer(t, change)
	f := &connFixture{t: t, s: s, clock: clock, player: uuid.NewSHA1(uuid.NameSpaceOID, []byte(t.Name()))}
	f.manifest = goldenBundle(t).Manifest
	s.OnClientMessage(func(player uuid.UUID, exp, channel string, schema uint16, payload []experience.Scalar) {
		f.mu.Lock()
		defer f.mu.Unlock()
		f.received = append(f.received, clientMessage{player, exp, channel, schema, describe(payload)})
	})
	f.inner = &fakeConn{identity: login.IdentityData{Identity: f.player.String(), DisplayName: "Steve"}}
	f.listener = &fakeListener{conns: []session.Conn{f.inner}}
	listen := s.Listener(func(server.Config) (server.Listener, error) { return f.listener, nil })
	l, err := listen(server.Config{})
	if err != nil {
		t.Fatal(err)
	}
	if f.conn, err = l.Accept(); err != nil {
		t.Fatal(err)
	}
	return f
}

func (f *connFixture) messages() []clientMessage {
	f.mu.Lock()
	defer f.mu.Unlock()
	return slices.Clone(f.received)
}

func (f *connFixture) encode(m Message) []byte {
	f.t.Helper()
	data, err := Encode(m)
	if err != nil {
		f.t.Fatal(err)
	}
	return data
}

// deliver sends each message on the carrier, then an ordinary packet, and checks that Dragonfly
// reads exactly that packet next: carrier messages never reach it and never end the connection.
func (f *connFixture) deliver(messages ...[]byte) {
	f.t.Helper()
	after := &packet.Text{Message: "after the carrier messages"}
	for _, m := range messages {
		f.inner.queue(&packet.ScriptMessage{Identifier: Carrier, Data: m})
	}
	f.inner.queue(after)
	if pk, err := f.conn.ReadPacket(); err != nil || pk != after {
		f.t.Fatalf("read %#v, %v; want the packet after the carrier messages", pk, err)
	}
	f.connected()
}

// connected checks that the player was never disconnected.
func (f *connFixture) connected() {
	f.t.Helper()
	f.inner.mu.Lock()
	closed := f.inner.closed
	f.inner.mu.Unlock()
	if closed || len(f.listener.disconnected) != 0 {
		f.t.Fatal("the player was disconnected")
	}
	for _, pk := range f.inner.written() {
		if _, ok := pk.(*packet.Disconnect); ok {
			f.t.Fatal("the player was sent a Disconnect")
		}
	}
}

// carrier returns the carrier messages written to the player, in order.
func (f *connFixture) carrier() [][]byte {
	var out [][]byte
	for _, pk := range f.inner.written() {
		if msg, ok := pk.(*packet.ScriptMessage); ok && msg.Identifier == Carrier {
			out = append(out, msg.Data)
		}
	}
	return out
}

// hello is a developer client's Hello for the server's offer, which offers every wire version
// at the client's ceilings.
func (f *connFixture) hello() Hello {
	h := f.helloV1()
	h.Wire = &WireOffer{Versions: []uint16{WireVersion, MaxWireVersion}, Limits: HostLimits}
	return h
}

// helloV1 is a v1 developer client's Hello, which offers no wire.
func (f *connFixture) helloV1() Hello {
	return Hello{
		Version:         WireVersion,
		API:             APIVersion,
		Capabilities:    ImplementedPermissions,
		OfferDigest:     f.s.digest,
		ClientChallenge: strings.Repeat("c1", 32),
		Connection:      strings.Repeat("c0", 32),
		Subclient:       0,
	}
}

// accept says h and returns the Accept the server replied with, verified as the client does.
func (f *connFixture) accept(h Hello) Accept {
	f.t.Helper()
	f.deliver(f.encode(Control{Hello: &h}))
	out := f.carrier()
	if len(out) != 1 {
		f.t.Fatalf("%d carrier messages after a hello, want an Accept", len(out))
	}
	var reply Control
	if err := Decode(out[0], &reply); err != nil || reply.Accept == nil {
		f.t.Fatalf("reply %s: %v", out[0], err)
	}
	var accept Accept
	if _, err := reply.Accept.Verify(f.s.offer.ServerKey, AcceptDomain, MaxPayloadBytes, &accept); err != nil {
		f.t.Fatalf("the Accept does not verify: %v", err)
	}
	return accept
}

// ready is the Ready of a developer client for accept: every bundle with its manifest
// permissions that the client implements.
func (f *connFixture) ready(accept Accept) Ready {
	return Ready{
		Session:     accept.Session,
		Packages:    []string{f.s.offer.Packages[0].Digest},
		Generation:  InitialBundleGeneration,
		Permissions: map[string]Permissions{f.manifest.ID: f.manifest.Permissions & ImplementedPermissions},
		WorldEpoch:  testEpoch,
	}
}

// activate completes a developer client's handshake.
func (f *connFixture) activate() Accept {
	f.t.Helper()
	accept := f.accept(f.hello())
	r := f.ready(accept)
	f.deliver(f.encode(Control{Ready: &r}))
	return accept
}

// channel is the manifest's channel in direction d.
func (f *connFixture) channel(d Direction) Channel {
	f.t.Helper()
	i := slices.IndexFunc(f.manifest.Channels, func(c Channel) bool { return c.Direction == d })
	if i < 0 {
		f.t.Fatalf("the golden manifest has no %v channel", d)
	}
	return f.manifest.Channels[i]
}

// envelope is the client part's to_server message number sequence, changed by change.
func (f *connFixture) envelope(accept Accept, sequence uint64, change func(*Envelope)) []byte {
	ch := f.channel(ToServer)
	e := Envelope{
		Version:    wireOf(accept).Version,
		Session:    accept.Session,
		Connection: accept.Hello.Connection,
		Subclient:  accept.Hello.Subclient,
		Bundle:     f.manifest.ID,
		Generation: InitialBundleGeneration,
		Channel:    ch.ID,
		Schema:     ch.Schema,
		Sequence:   sequence,
		WorldEpoch: testEpoch,
		Payload:    []experience.Scalar{integer(int64(sequence))},
	}
	if change != nil {
		change(&e)
	}
	return f.encode(e)
}

// wireOf is the wire that accept selected.
func wireOf(accept Accept) Wire {
	if accept.Wire == nil {
		return v1Wire
	}
	return *accept.Wire
}

// sendToClient sends n on the manifest's to_client channel.
func (f *connFixture) sendToClient(n int64) bool {
	ch := f.channel(ToClient)
	return f.s.Send(f.player, f.manifest.ID, ch.ID, ch.Schema, []experience.Scalar{integer(n)})
}

// fellBack checks the connection is in fallback: nothing more reaches the client part, nothing
// more reaches the server's Experiences and the player stays connected.
func (f *connFixture) fellBack() {
	f.t.Helper()
	sent, received := len(f.carrier()), len(f.messages())
	if f.sendToClient(1) || len(f.carrier()) != sent {
		f.t.Fatal("a message went to a client part in fallback")
	}
	f.deliver(f.envelope(Accept{}, 1, nil))
	if len(f.messages()) != received {
		f.t.Fatal("a client part in fallback reached the server")
	}
	f.connected()
}

// A developer client's Hello gets a signed Accept bound to that Hello; after its Ready, typed
// messages flow both ways, sequenced from 1 with Ready's world epoch, and every other packet,
// including other script messages, reaches Dragonfly untouched.
func TestHandshakeCarriesBothDirections(t *testing.T) {
	f := newConn(t, nil)
	if f.sendToClient(1) {
		t.Fatal("sent before the handshake")
	}
	h := f.hello()
	accept := f.accept(h)
	offer, digest := markerOffer(t, f.s.Marker())
	now := uint64(f.clock.Now().Unix())
	switch {
	case !reflect.DeepEqual(accept.Hello, h):
		t.Fatalf("Accept echoes %+v, not the Hello %+v", accept.Hello, h)
	case accept.Audience != offer.Audience || accept.OfferDigest != digest || accept.Revision != offer.Revision:
		t.Fatalf("Accept for %q, offer %s, revision %d", accept.Audience, accept.OfferDigest, accept.Revision)
	case accept.ExpiresUnix <= now || accept.ExpiresUnix > offer.ExpiresUnix:
		t.Fatalf("Accept expires %d at %d, offer at %d", accept.ExpiresUnix, now, offer.ExpiresUnix)
	}
	for _, nonce := range []string{accept.ServerChallenge, accept.Session} {
		if _, err := fixedHex(nonce, 32); err != nil {
			t.Fatalf("nonce %q: %v", nonce, err)
		}
	}
	if accept.ServerChallenge == accept.Session {
		t.Fatal("the challenge and the session are the same nonce")
	}
	if want := (Wire{MaxWireVersion, HostLimits}); accept.Wire == nil || *accept.Wire != want {
		t.Fatalf("Accept selects %+v, want %+v", accept.Wire, want)
	}
	if f.sendToClient(1) {
		t.Fatal("sent before Ready")
	}
	r := f.ready(accept)
	f.deliver(f.encode(Control{Ready: &r}))

	toClient := f.channel(ToClient)
	for sequence := uint64(1); sequence <= 2; sequence++ {
		if !f.sendToClient(int64(10 * sequence)) {
			t.Fatalf("send %d refused", sequence)
		}
		out := f.carrier()
		want := f.encode(Envelope{
			Version:    MaxWireVersion,
			Session:    accept.Session,
			Connection: h.Connection,
			Subclient:  h.Subclient,
			Bundle:     f.manifest.ID,
			Generation: InitialBundleGeneration,
			Channel:    toClient.ID,
			Schema:     toClient.Schema,
			Sequence:   sequence,
			WorldEpoch: testEpoch,
			Payload:    []experience.Scalar{integer(int64(10 * sequence))},
		})
		if got := out[len(out)-1]; string(got) != string(want) {
			t.Fatalf("envelope %s, want %s", got, want)
		}
	}

	f.deliver(f.envelope(accept, 1, nil), f.envelope(accept, 2, nil))
	toServer := f.channel(ToServer)
	want := []clientMessage{
		{f.player, f.manifest.ID, toServer.ID, toServer.Schema, describe([]experience.Scalar{integer(1)})},
		{f.player, f.manifest.ID, toServer.ID, toServer.Schema, describe([]experience.Scalar{integer(2)})},
	}
	if got := f.messages(); !slices.Equal(got, want) {
		t.Fatalf("received %+v, want %+v", got, want)
	}

	other := &packet.ScriptMessage{Identifier: "other:messages", Data: []byte("{}")}
	f.inner.queue(other)
	if pk, err := f.conn.ReadPacket(); err != nil || pk != other {
		t.Fatalf("read %#v, %v; want the other script message untouched", pk, err)
	}
	f.connected()
}

// A Hello the server cannot accept gets no Accept, and the connection stays in fallback even if a
// good Hello follows.
func TestHelloViolationsFallBack(t *testing.T) {
	for _, c := range []struct {
		name    string
		message func(*connFixture) []byte
	}{
		{"another offer", changedHello(func(h *Hello) { h.OfferDigest = strings.Repeat("00", 32) })},
		{"another version", changedHello(func(h *Hello) { h.Version++ })},
		{"another api", changedHello(func(h *Hello) { h.API++ })},
		{"subclient 1", changedHello(func(h *Hello) { h.Subclient = 1 })},
		{"no capabilities", changedHello(func(h *Hello) { h.Capabilities = 0 })},
		{"a short challenge", changedHello(func(h *Hello) { h.ClientChallenge = "c1" })},
		{"a connection in upper case", changedHello(func(h *Hello) { h.Connection = strings.ToUpper(h.Connection) })},
		{"no common wire version", changedHello(func(h *Hello) { h.Wire.Versions = []uint16{MaxWireVersion + 1} })},
		{"no wire versions", changedHello(func(h *Hello) { h.Wire.Versions = nil })},
		{"a zero fragment limit", changedHello(func(h *Hello) { h.Wire.Limits.MaxFragmentBytes = 0 })},
		{"a message limit below the fragment limit", changedHello(func(h *Hello) { h.Wire.Limits.MaxMessageBytes = 1 })},
		{"Ready first", func(f *connFixture) []byte {
			r := f.ready(Accept{Session: strings.Repeat("00", 32)})
			return f.encode(Control{Ready: &r})
		}},
		{"an envelope first", func(f *connFixture) []byte { return f.envelope(Accept{}, 1, nil) }},
		{"no JSON", func(*connFixture) []byte { return []byte("{") }},
	} {
		t.Run(c.name, func(t *testing.T) {
			f := newConn(t, nil)
			f.deliver(c.message(f))
			h := f.hello()
			f.deliver(f.encode(Control{Hello: &h}))
			if n := len(f.carrier()); n != 0 {
				t.Fatalf("%d carrier messages; want no Accept", n)
			}
			f.fellBack()
		})
	}
}

// changedHello is a developer client's Hello changed by change.
func changedHello(change func(*Hello)) func(*connFixture) []byte {
	return func(f *connFixture) []byte {
		h := f.hello()
		change(&h)
		return f.encode(Control{Hello: &h})
	}
}

// A connection gets one Accept: a second Hello is a violation, and no Ready revives it.
func TestAcceptIsSentOnce(t *testing.T) {
	f := newConn(t, nil)
	accept := f.accept(f.hello())
	h := f.hello()
	f.deliver(f.encode(Control{Hello: &h}))
	if n := len(f.carrier()); n != 1 {
		t.Fatalf("%d carrier messages after two hellos, want one Accept", n)
	}
	r := f.ready(accept)
	f.deliver(f.encode(Control{Ready: &r}))
	f.fellBack()
}

// A Ready that does not match the Accept, the offer or what the client may run is a violation, and
// a correct Ready after it does not revive the connection.
func TestReadyMismatchFallsBack(t *testing.T) {
	everything := NewPermissions(PermissionUI, PermissionModalUI, PermissionInput, PermissionMessaging, PermissionScene, PermissionMedia)
	for _, c := range []struct {
		name   string
		hello  func(*Hello)
		change func(*connFixture, *Ready)
	}{
		{name: "another session", change: func(_ *connFixture, r *Ready) { r.Session = strings.Repeat("00", 32) }},
		{name: "another package", change: func(_ *connFixture, r *Ready) { r.Packages = []string{strings.Repeat("00", 32)} }},
		{name: "no packages", change: func(_ *connFixture, r *Ready) { r.Packages = nil }},
		{name: "an extra package", change: func(_ *connFixture, r *Ready) { r.Packages = append(r.Packages, r.Packages[0]) }},
		{name: "generation 2", change: func(_ *connFixture, r *Ready) { r.Generation = InitialBundleGeneration + 1 }},
		{
			name:   "a permission the Hello lacks",
			hello:  func(h *Hello) { h.Capabilities = NewPermissions(PermissionUI) },
			change: func(*connFixture, *Ready) {},
		},
		{
			name:  "a permission the manifest lacks",
			hello: func(h *Hello) { h.Capabilities = everything },
			change: func(f *connFixture, r *Ready) {
				r.Permissions[f.manifest.ID] |= everything &^ f.manifest.Permissions
			},
		},
		{name: "an unknown bundle", change: func(_ *connFixture, r *Ready) { r.Permissions["other"] = NewPermissions(PermissionUI) }},
		{name: "a missing bundle", change: func(_ *connFixture, r *Ready) { r.Permissions = map[string]Permissions{} }},
	} {
		t.Run(c.name, func(t *testing.T) {
			f := newConn(t, nil)
			h := f.hello()
			if c.hello != nil {
				c.hello(&h)
			}
			accept := f.accept(h)
			r := f.ready(accept)
			c.change(f, &r)
			f.deliver(f.encode(Control{Ready: &r}))
			f.fellBack()
			r = f.ready(accept)
			f.deliver(f.encode(Control{Ready: &r}))
			f.fellBack()
		})
	}
}

// Nothing but Ready may follow the Accept.
func TestEnvelopeBeforeReadyFallsBack(t *testing.T) {
	f := newConn(t, nil)
	accept := f.accept(f.hello())
	f.deliver(f.envelope(accept, 1, nil))
	f.fellBack()
	r := f.ready(accept)
	f.deliver(f.encode(Control{Ready: &r}))
	f.fellBack()
}

// Inbound envelopes are checked by the client's own ingress rules against the manifest's to_server
// channels; the first one that breaks them puts the connection in fallback. The server knows the
// client part's exact manifest, so an undeclared channel or schema is a violation here, not a
// newer revision to skip.
func TestInboundEnvelopeViolationsFallBack(t *testing.T) {
	toClient := func(f *connFixture) Channel { return f.channel(ToClient) }
	for _, c := range []struct {
		name      string
		messages  func(*connFixture, Accept) [][]byte
		delivered int
	}{
		{"a replay", func(f *connFixture, a Accept) [][]byte { return [][]byte{f.envelope(a, 1, nil), f.envelope(a, 1, nil)} }, 1},
		{"a gap", func(f *connFixture, a Accept) [][]byte { return [][]byte{f.envelope(a, 2, nil)} }, 0},
		{"an oversized envelope", func(f *connFixture, a Accept) [][]byte {
			return [][]byte{f.envelope(a, 1, func(e *Envelope) {
				e.Payload = []experience.Scalar{text(strings.Repeat("x", MaxEnvelopeBytes))}
			})}
		}, 0},
		{"an unknown channel", func(f *connFixture, a Accept) [][]byte {
			return [][]byte{f.envelope(a, 1, func(e *Envelope) { e.Channel = f.manifest.ID + ".unknown" })}
		}, 0},
		{"an unknown schema", func(f *connFixture, a Accept) [][]byte {
			return [][]byte{f.envelope(a, 1, func(e *Envelope) { e.Schema++ })}
		}, 0},
		{"a to_client channel", func(f *connFixture, a Accept) [][]byte {
			return [][]byte{f.envelope(a, 1, func(e *Envelope) { e.Channel, e.Schema = toClient(f).ID, toClient(f).Schema })}
		}, 0},
		{"another namespace", func(f *connFixture, a Accept) [][]byte {
			return [][]byte{f.envelope(a, 1, func(e *Envelope) { e.Channel = "other" + strings.TrimPrefix(e.Channel, e.Bundle) })}
		}, 0},
		{"another session", func(f *connFixture, a Accept) [][]byte {
			return [][]byte{f.envelope(a, 1, func(e *Envelope) { e.Session = strings.Repeat("00", 32) })}
		}, 0},
		{"a record outside the schema", func(f *connFixture, a Accept) [][]byte {
			return [][]byte{f.envelope(a, 1, func(e *Envelope) { e.Payload = []experience.Scalar{integer(-1)} })}
		}, 0},
		{"a control message", func(f *connFixture, a Accept) [][]byte {
			r := f.ready(a)
			return [][]byte{f.encode(Control{Ready: &r})}
		}, 0},
		{"a v1 envelope", func(f *connFixture, a Accept) [][]byte {
			return [][]byte{f.envelope(a, 1, func(e *Envelope) { e.Version = WireVersion })}
		}, 0},
	} {
		t.Run(c.name, func(t *testing.T) {
			f := newConn(t, nil)
			accept := f.activate()
			f.deliver(c.messages(f, accept)...)
			if n := len(f.messages()); n != c.delivered {
				t.Fatalf("%d messages delivered, want %d", n, c.delivered)
			}
			f.fellBack()
		})
	}
}

// Closing the connection ends its client part; Dragonfly's Disconnect reaches the listener with
// the connection that listener accepted, which its default RakNet listener requires.
func TestDisconnectEndsTheClientPart(t *testing.T) {
	f := newConn(t, nil)
	f.activate()
	if err := f.conn.Close(); err != nil {
		t.Fatal(err)
	}
	if !f.inner.closed {
		t.Fatal("Close did not reach the connection")
	}
	if f.sendToClient(1) {
		t.Fatal("sent after the connection closed")
	}

	g := newConn(t, nil)
	g.activate()
	listen := g.s.Listener(func(server.Config) (server.Listener, error) { return g.listener, nil })
	l, err := listen(server.Config{})
	if err != nil {
		t.Fatal(err)
	}
	if err := l.Disconnect(g.conn, "bye"); err != nil {
		t.Fatal(err)
	}
	if len(g.listener.disconnected) != 1 || g.listener.disconnected[0] != session.Conn(g.inner) {
		t.Fatalf("the listener disconnected %v, want the connection it accepted", g.listener.disconnected)
	}
	if g.sendToClient(1) {
		t.Fatal("sent after Disconnect")
	}
	if err := l.Close(); err != nil || !g.listener.closed {
		t.Fatalf("Close: %v; the listener closed: %v", err, g.listener.closed)
	}
	if _, err := l.Accept(); err == nil {
		t.Fatal("Accept after the listener's last connection")
	}
}

// A dimension change, which resets a v1 client's world epoch, ends its active client part; before
// a Hello there is no session for it to end.
func TestDimensionChangeEndsV1ClientParts(t *testing.T) {
	change := &packet.ChangeDimension{Dimension: 1}
	f := newConn(t, nil)
	accept := f.accept(f.helloV1())
	r := f.ready(accept)
	f.deliver(f.encode(Control{Ready: &r}))
	if err := f.conn.WritePacket(change); err != nil {
		t.Fatal(err)
	}
	if out := f.inner.written(); out[len(out)-1] != change {
		t.Fatal("the dimension change did not reach the player")
	}
	f.fellBack()

	g := newConn(t, nil)
	if err := g.conn.WritePacket(change); err != nil {
		t.Fatal(err)
	}
	g.activate()
	if !g.sendToClient(1) {
		t.Fatal("a dimension change before the Hello ended the client part")
	}
}

// At its Accept's expiry the client part ends; a Hello after the offer's expiry gets no Accept.
func TestExpiryFallsBack(t *testing.T) {
	f := newConn(t, nil)
	accept := f.activate()
	f.clock.Advance(time.Duration(accept.ExpiresUnix-uint64(f.clock.Now().Unix())) * time.Second)
	f.deliver(f.envelope(accept, 1, nil))
	if n := len(f.messages()); n != 0 {
		t.Fatalf("%d messages delivered after the Accept expired", n)
	}
	f.fellBack()

	g := newConn(t, nil)
	g.clock.Advance(time.Duration(g.s.offer.ExpiresUnix-uint64(g.clock.Now().Unix())) * time.Second)
	h := g.hello()
	g.deliver(g.encode(Control{Hello: &h}))
	if n := len(g.carrier()); n != 0 {
		t.Fatalf("%d carrier messages after an expired offer's Hello", n)
	}
	g.fellBack()
}

// Send refuses what the client part would not take, without consuming a sequence number or ending
// the client part.
func TestSendRefusesWithoutFallingBack(t *testing.T) {
	f := newConn(t, nil)
	f.activate()
	toClient, toServer := f.channel(ToClient), f.channel(ToServer)
	one := []experience.Scalar{integer(1)}
	for _, c := range []struct {
		name    string
		player  uuid.UUID
		exp     string
		channel string
		schema  uint16
		payload []experience.Scalar
	}{
		{"an undeclared channel", f.player, f.manifest.ID, f.manifest.ID + ".unknown", toClient.Schema, one},
		{"another schema", f.player, f.manifest.ID, toClient.ID, toClient.Schema + 1, one},
		{"a to_server channel", f.player, f.manifest.ID, toServer.ID, toServer.Schema, one},
		{"a record outside the schema", f.player, f.manifest.ID, toClient.ID, toClient.Schema, []experience.Scalar{integer(-1)}},
		{"another Experience", f.player, "other", toClient.ID, toClient.Schema, one},
		{"another player", uuid.New(), f.manifest.ID, toClient.ID, toClient.Schema, one},
	} {
		if f.s.Send(c.player, c.exp, c.channel, c.schema, c.payload) {
			t.Fatalf("sent %s", c.name)
		}
	}
	if !f.sendToClient(1) {
		t.Fatal("refusals ended the client part")
	}
	var e Envelope
	if out := f.carrier(); len(out) != 2 || Decode(out[1], &e) != nil || e.Sequence != 1 {
		t.Fatalf("carrier %q; want the Accept and envelope 1", out)
	}
}

// A bundle that Ready did not grant messaging gets no messages.
func TestSendNeedsTheMessagingGrant(t *testing.T) {
	f := newConn(t, nil)
	accept := f.accept(f.hello())
	r := f.ready(accept)
	r.Permissions[f.manifest.ID] &^= NewPermissions(PermissionMessaging)
	f.deliver(f.encode(Control{Ready: &r}))
	if f.sendToClient(1) {
		t.Fatal("sent to a bundle without messaging")
	}
}

// The server never sends faster than the client's ingress takes: a send over the rate is refused
// and consumes no sequence number.
func TestSendKeepsToTheClientRate(t *testing.T) {
	f := newConn(t, nil)
	f.activate()
	for i := range MaxMessagesPerSecond {
		if !f.sendToClient(1) {
			t.Fatalf("send %d refused", i+1)
		}
	}
	if f.sendToClient(1) {
		t.Fatal("sent over the client's message rate")
	}
	f.clock.Advance(time.Second)
	if !f.sendToClient(1) {
		t.Fatal("refused after the rate recovered")
	}
	for i, data := range f.carrier()[1:] {
		var e Envelope
		if err := Decode(data, &e); err != nil || e.Sequence != uint64(i+1) {
			t.Fatalf("envelope %d has sequence %d: %v", i+1, e.Sequence, err)
		}
	}
}

// The server declares exactly the bundles' to_client channel schemas, by Experience id.
func TestDeclaresTheToClientChannels(t *testing.T) {
	s, _ := testServer(t, nil)
	manifest := goldenBundle(t).Manifest
	for _, c := range manifest.Channels {
		if got := s.Declares(manifest.ID, c.ID, c.Schema); got != (c.Direction == ToClient) {
			t.Errorf("Declares(%s, %s, %d) = %v", manifest.ID, c.ID, c.Schema, got)
		}
		if s.Declares(manifest.ID, c.ID, c.Schema+1) || s.Declares("other", c.ID, c.Schema) {
			t.Errorf("declares another schema or Experience for %s", c.ID)
		}
	}
}
