package experience

import (
	"crypto/ed25519"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	ext "github.com/cinnabar-mc/cinnabar/tools/localserver/cinnabarext"
	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// lifecycleConn supplies a synchronous packet stream to the real extension listener.
type lifecycleConn struct {
	session.Conn
	id      uuid.UUID
	in, out []packet.Packet
}

// IdentityData binds the fixture connection to its server-side player.
func (c *lifecycleConn) IdentityData() login.IdentityData {
	return login.IdentityData{Identity: c.id.String(), DisplayName: "Actor"}
}

// WritePacket records extension controls without a socket.
func (c *lifecycleConn) WritePacket(pk packet.Packet) error { c.out = append(c.out, pk); return nil }

// ReadPacket consumes one packet supplied by the test.
func (c *lifecycleConn) ReadPacket() (packet.Packet, error) {
	if len(c.in) == 0 {
		return nil, errors.New("empty fixture stream")
	}
	pk := c.in[0]
	c.in = c.in[1:]
	return pk, nil
}

// Close leaves the in-memory transport available for assertions.
func (*lifecycleConn) Close() error { return nil }

// lifecycleListener admits one fixture connection through the production wrapper.
type lifecycleListener struct {
	server.Listener
	c session.Conn
}

// Accept returns the connection without opening a network listener.
func (l lifecycleListener) Accept() (session.Conn, error) { return l.c, nil }

// lifecyclePart exercises real signed activation and transfer fences inside the host matrix.
type lifecyclePart struct {
	t                  *testing.T
	server             *ext.Server
	now                *time.Time
	conn               session.Conn
	inner              *lifecycleConn
	hello              ext.Hello
	accept             ext.Accept
	ready              bool
	sequence           uint64
	admitted           int
	incoming           ext.Channel
	acknowledgedNotice uint64
}

// readExtensionFixture uses the shared protocol vectors rather than copying their fields.
func readExtensionFixture(t *testing.T, name string, target ext.Target) {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("..", "cinnabarext", "testdata", name))
	if err != nil {
		t.Fatal(err)
	}
	if err := ext.Decode(data, target); err != nil {
		t.Fatal(err)
	}
}

// newLifecyclePart constructs an optional client part without a native client or socket.
func newLifecyclePart(t *testing.T, f *runtimeFixture, existing ...*lifecyclePart) *lifecyclePart {
	t.Helper()
	var manifest ext.Manifest
	readExtensionFixture(t, "manifest_payload.json", &manifest)
	originalID := manifest.ID
	manifest.ID = "probe"
	for i := range manifest.Channels {
		manifest.Channels[i].ID = manifest.ID + strings.TrimPrefix(manifest.Channels[i].ID, originalID)
	}
	var offer ext.Offer
	readExtensionFixture(t, "offer_payload.json", &offer)
	now := time.Unix(int64(offer.ExpiresUnix)-1, 0)
	clock := &now
	if len(existing) != 0 {
		clock = existing[0].now
	}
	s, err := ext.New(ext.Config{Key: ed25519.NewKeyFromSeed(make([]byte, ed25519.SeedSize)), Audience: offer.Audience, Now: func() time.Time { return *clock }, Bundles: []ext.Bundle{{Manifest: manifest, Digest: offer.Packages[0].Digest, Bytes: offer.Packages[0].Bytes}}})
	if err != nil {
		t.Fatal(err)
	}
	if len(existing) != 0 {
		s = existing[0].server
	}
	part := &lifecyclePart{t: t, server: s, now: clock, sequence: 1}
	for _, channel := range manifest.Channels {
		if channel.Direction == ext.ToServer {
			part.incoming = channel
			break
		}
	}
	part.inner = &lifecycleConn{id: f.actor.UUID()}
	listen := s.Listener(func(server.Config) (server.Listener, error) { return lifecycleListener{c: part.inner}, nil })
	listener, err := listen(server.Config{})
	if err != nil {
		t.Fatal(err)
	}
	part.conn, err = listener.Accept()
	if err != nil {
		t.Fatal(err)
	}
	if err := part.conn.WritePacket(&packet.ItemRegistry{}); err != nil {
		t.Fatal(err)
	}
	readExtensionFixture(t, "hello_payload.json", &part.hello)
	encoded, err := ext.Encode(s.Offer())
	if err != nil {
		t.Fatal(err)
	}
	part.hello.OfferDigest = ext.Digest(encoded)
	part.hello.Wire = &ext.WireOffer{Versions: []uint16{ext.MaxWireVersion}, Limits: ext.HostLimits}
	s.OnClientMessage(func(_ uuid.UUID, _ ext.PlayerSession, exp, channel string, schema uint16, payload []ext.Scalar) {
		if f.h.DeliverClientMessage(f.actor, exp, channel, schema, payload) {
			part.admitted++
		}
	})
	f.h.channels = s
	if state := f.h.players.items[f.actor]; state != nil {
		state.connection = s.PlayerSession(f.actor.UUID())
	}
	return part
}

// deliver drives the actual carrier decoder through one complete synchronous packet read.
func (p *lifecyclePart) deliver(value ext.CanonicalValue) {
	p.t.Helper()
	encoded, err := ext.Encode(value)
	if err != nil {
		p.t.Fatal(err)
	}
	after := &packet.Text{}
	p.inner.in = append(p.inner.in, &packet.ScriptMessage{Identifier: ext.Carrier, Data: encoded}, after)
	if pk, err := p.conn.ReadPacket(); err != nil || pk != after {
		p.t.Fatalf("carrier delivery: %v, %v", pk, err)
	}
}

// handshake completes the signed Hello/Ready exchange once.
func (p *lifecyclePart) handshake() {
	if p.ready {
		return
	}
	p.deliver(ext.Control{Hello: &p.hello})
	var reply ext.Control
	pk := p.inner.out[len(p.inner.out)-1].(*packet.ScriptMessage)
	if err := ext.Decode(pk.Data, &reply); err != nil || reply.Accept == nil {
		p.t.Fatalf("accept: %v", err)
	}
	if _, err := reply.Accept.Verify(p.server.Offer().ServerKey, ext.AcceptDomain, ext.MaxPayloadBytes, &p.accept); err != nil {
		p.t.Fatal(err)
	}
	var control ext.Control
	readExtensionFixture(p.t, "ready_message.json", &control)
	r := control.Ready
	r.Session = p.accept.Session
	r.Packages = []string{p.server.Offer().Packages[0].Digest}
	r.Permissions = map[string]ext.Permissions{"probe": ext.NewPermissions(ext.PermissionMessaging)}
	p.deliver(ext.Control{Ready: r})
	p.ready = true
}

// oldInput probes the original Ready epoch without acknowledging any destination notice.
func (p *lifecyclePart) oldInput() { p.inputAt(0) }

// inputAt sends a record in the requested epoch; zero selects the original Ready epoch.
func (p *lifecyclePart) inputAt(epoch uint64) {
	if !p.ready {
		return
	}
	var e ext.Envelope
	readExtensionFixture(p.t, "envelope_to_server.json", &e)
	e.Version = ext.WireVersion
	if p.accept.Wire != nil {
		e.Version = p.accept.Wire.Version
	}
	e.Session = p.accept.Session
	e.Connection = p.hello.Connection
	e.Bundle = "probe"
	e.Channel, e.Schema = p.incoming.ID, p.incoming.Schema
	if epoch != 0 {
		e.WorldEpoch = epoch
	}
	e.Sequence = p.sequence
	p.sequence++
	p.deliver(e)
}

// acknowledgeWorld completes the oldest unread notice and returns its client epoch.
func (p *lifecyclePart) acknowledgeWorld() uint64 {
	p.t.Helper()
	for _, pk := range p.inner.out {
		msg, ok := pk.(*packet.ScriptMessage)
		if !ok || msg.Identifier != ext.Carrier {
			continue
		}
		var control ext.Control
		if err := ext.Decode(msg.Data, &control); err != nil {
			p.t.Fatal(err)
		}
		if epoch := control.Epoch; epoch != nil && epoch.WorldEpoch > p.acknowledgedNotice {
			p.acknowledgedNotice = epoch.WorldEpoch
			p.deliver(ext.Control{WorldAck: &ext.WorldAck{Session: epoch.Session, WorldEpoch: epoch.WorldEpoch, RequestedEpoch: epoch.WorldEpoch}})
			return epoch.WorldEpoch
		}
	}
	p.t.Fatal("no unacknowledged world notice")
	return 0
}

// TestLifecycleClientTransferFence admits source traffic and rejects it after same-type travel.
func TestLifecycleClientTransferFence(t *testing.T) {
	f := newRuntimeFixture(t)
	part := newLifecyclePart(t, f)
	part.handshake()
	part.oldInput()
	if part.admitted != 1 {
		t.Fatal("source input did not reach the host")
	}
	if err := f.travel(t.Context(), "probe:one"); err != nil {
		t.Fatal(err)
	}
	part.oldInput()
	if part.admitted != 1 || f.countLocations() != 1 {
		t.Fatal("old input crossed the committed transfer")
	}
}
