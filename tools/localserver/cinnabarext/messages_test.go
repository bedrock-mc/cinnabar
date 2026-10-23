package cinnabarext

import (
	"testing"

	"github.com/google/uuid"
)

// testMessage encodes the integer fixture channel as a typed application message.
type testMessage struct {
	route  Route
	number int64
}

// Route supplies this fixture's declared channel.
func (m testMessage) Route() Route { return m.route }

// Encode supplies one integer field for schema validation.
func (m testMessage) Encode() (Record, error) { return Record{integer(m.number)}, nil }

// readyFixture completes the signed v2 handshake without changing its clock.
func readyFixture(t *testing.T) (*connFixture, Accept) {
	t.Helper()
	f := newConn(t, nil)
	accept := f.accept(f.hello())
	ready := f.ready(accept)
	f.deliver(f.encode(Control{Ready: &ready}))
	return f, accept
}

func TestTypedSendBroadcastAndReceive(t *testing.T) {
	f, accept := readyFixture(t)
	outgoing, incoming := f.channel(ToClient), f.channel(ToServer)
	var sender uuid.UUID
	var number int64
	kind := MessageType[int64]{Route: Route{f.manifest.ID, incoming.ID, incoming.Schema}, Decode: func(record Record) (int64, error) { return *record[0].Integer, nil }}
	if err := OnMessage(f.s, kind, func(id uuid.UUID, value int64) { sender, number = id, value }); err != nil {
		t.Fatal(err)
	}
	if err := OnMessage(f.s, kind, func(uuid.UUID, int64) {}); err == nil {
		t.Fatal("duplicate handler accepted")
	}
	f.deliver(f.envelope(accept, 1, nil))
	if sender != f.player || number != 1 {
		t.Fatalf("received %v %d", sender, number)
	}
	message := testMessage{Route{f.manifest.ID, outgoing.ID, outgoing.Schema}, 7}
	if !f.s.Send(f.player, message) {
		t.Fatal("typed send rejected")
	}
	if sent := f.s.Broadcast([]uuid.UUID{uuid.Nil, f.player}, message); sent != 1 {
		t.Fatalf("broadcast admitted %d players", sent)
	}
	message.number = -1
	if f.s.Send(f.player, message) {
		t.Fatal("invalid typed payload sent")
	}
}

func TestWorldChangedFencesMessagesUntilAcknowledgment(t *testing.T) {
	f, accept := readyFixture(t)
	var epochs int
	f.s.OnEpoch(func(id uuid.UUID, _ string) {
		if id == f.player {
			epochs++
		}
	})
	if !f.s.WorldChanged(f.player, "sample:first") {
		t.Fatal("world notice rejected")
	}
	out := f.carrier()
	var notice Control
	if err := Decode(out[len(out)-1], &notice); err != nil || notice.Epoch == nil {
		t.Fatalf("world notice: %v", err)
	}
	if notice.Epoch.Session != accept.Session || notice.Epoch.WorldEpoch != testEpoch+1 {
		t.Fatalf("world notice %+v", notice.Epoch)
	}
	if f.sendToClient(7) {
		t.Fatal("send crossed the world fence")
	}
	f.deliver(f.envelope(accept, 1, nil))
	if len(f.messages()) != 0 {
		t.Fatal("old-world message reached the application")
	}
	f.deliver(f.encode(worldAcknowledgment(notice, notice.Epoch.WorldEpoch)))
	if epochs != 1 || !f.sendToClient(8) {
		t.Fatal("world acknowledgment did not reopen sends")
	}
	before := len(f.carrier())
	if !f.s.WorldChanged(f.player, "sample:first") || len(f.carrier()) != before {
		t.Fatal("same world was not idempotent")
	}
	if !f.s.WorldChanged(f.player, "sample:second") || f.sendToClient(9) {
		t.Fatal("second world did not establish a fence")
	}
}

func TestWorldChangedDoesNotProbeInactiveOrV1Clients(t *testing.T) {
	f := newConn(t, nil)
	if f.s.WorldChanged(f.player, "sample:first") || f.s.WorldChanged(uuid.Nil, "sample:first") {
		t.Fatal("inactive player notified")
	}
	accept := f.accept(f.helloV1())
	ready := f.ready(accept)
	f.deliver(f.encode(Control{Ready: &ready}))
	if f.s.WorldChanged(f.player, "sample:first") {
		t.Fatal("v1 player notified")
	}
	if !f.sendToClient(7) {
		t.Fatal("ordinary v1 send changed")
	}
}

func TestAllowedCapabilitiesRejectUnexpectedBundleAuthority(t *testing.T) {
	cfg, _ := testConfig(t)
	allowed := Permissions(0)
	cfg.AllowedCapabilities = &allowed
	if _, err := New(cfg); err == nil {
		t.Fatal("bundle authority exceeded operator policy")
	}
}
