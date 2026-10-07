package extension

import (
	"fmt"
	"reflect"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/experience"
)

// itemsChannel is the golden manifest's list channel.
func (f *connFixture) itemsChannel() Channel {
	f.t.Helper()
	i := slices.IndexFunc(f.manifest.Channels, func(c Channel) bool { return c.ID == f.manifest.ID+".items" })
	if i < 0 {
		f.t.Fatal("the golden manifest has no items channel")
	}
	return f.manifest.Channels[i]
}

// itemsRecord is a record of the items channel holding n items with multi-byte names.
func itemsRecord(n int) []experience.Scalar {
	var all []experience.Scalar
	for i := range n {
		all = append(all, recordOf(
			text(fmt.Sprintf("ae2:item_%d", i)),
			integer(int64(i)),
			text(fmt.Sprintf("Certus Quartz Crystal \u00e9\U0001F600 #%d", i)),
			listOf(recordOf(boolean(i%2 == 0), choice(uint16(i%4)))),
		))
	}
	return []experience.Scalar{listOf(all...), recordOf()}
}

// clientGrant is how the client's ingress sees the session that f's server accepted.
func (f *connFixture) clientGrant(accept Accept) *Grant {
	return &Grant{
		Offer:      &f.s.offer,
		Session:    accept.Session,
		Connection: accept.Hello.Connection,
		Subclient:  accept.Hello.Subclient,
		Wire:       wireOf(accept),
		Recipients: map[string]Recipient{f.manifest.ID: {
			Permissions: f.ready(accept).Permissions[f.manifest.ID],
			Channels:    f.manifest.Channels,
		}},
	}
}

// v1 and v2 Hellos get the highest version both sides speak, at the lower of each ceiling. A v1
// Hello gets exactly the v1 handshake: an Accept that selects nothing and v1 envelopes.
func TestWireNegotiationMatrix(t *testing.T) {
	lower := Limits{1024, 4096, 8192}
	for _, c := range []struct {
		name  string
		offer *WireOffer
		want  *Wire
	}{
		{"a v1 client", nil, nil},
		{"a v2 client", &WireOffer{[]uint16{WireVersion, MaxWireVersion}, HostLimits}, &Wire{MaxWireVersion, HostLimits}},
		{"a client of v1 only", &WireOffer{[]uint16{WireVersion}, HostLimits}, nil},
		{"a client of a later version too", &WireOffer{[]uint16{WireVersion, MaxWireVersion, MaxWireVersion + 1}, HostLimits}, &Wire{MaxWireVersion, HostLimits}},
		{"a client with lower ceilings", &WireOffer{[]uint16{MaxWireVersion}, lower}, &Wire{MaxWireVersion, lower}},
		{"a client with higher ceilings", &WireOffer{[]uint16{MaxWireVersion}, Limits{1 << 30, 1 << 30, 1 << 30}}, &Wire{MaxWireVersion, HostLimits}},
	} {
		t.Run(c.name, func(t *testing.T) {
			f := newConn(t, nil)
			h := f.helloV1()
			h.Wire = c.offer
			accept := f.accept(h)
			if !reflect.DeepEqual(accept.Wire, c.want) {
				t.Fatalf("Accept selects %+v, want %+v", accept.Wire, c.want)
			}
			r := f.ready(accept)
			f.deliver(f.encode(Control{Ready: &r}))
			if !f.sendToClient(7) {
				t.Fatal("send refused")
			}
			out := f.carrier()
			if _, err := receiveAll(NewIngress(ToClient, 0), f.clientGrant(accept), testEpoch, out[1:]); err != nil {
				t.Fatalf("the client's ingress refuses %s: %v", out[1], err)
			}
			f.deliver(f.envelope(accept, 1, nil))
			if n := len(f.messages()); n != 1 {
				t.Fatalf("%d messages delivered", n)
			}
		})
	}
}

// A v1 client keeps one world epoch for its session, so its envelope of another one is a
// violation; it has no epoch control either.
func TestV1KeepsOneWorldEpoch(t *testing.T) {
	for _, c := range []struct {
		name    string
		message func(*connFixture, Accept) []byte
	}{
		{"an envelope of another epoch", func(f *connFixture, a Accept) []byte {
			return f.envelope(a, 1, func(e *Envelope) { e.WorldEpoch++ })
		}},
		{"an epoch control", func(f *connFixture, a Accept) []byte {
			return f.encode(Control{Epoch: &Epoch{a.Session, testEpoch + 1}})
		}},
	} {
		t.Run(c.name, func(t *testing.T) {
			f := newConn(t, nil)
			accept := f.accept(f.helloV1())
			r := f.ready(accept)
			f.deliver(f.encode(Control{Ready: &r}), c.message(f, accept))
			f.fellBack()
		})
	}
}

// On wire v2 a dimension change keeps the client part. The client's epoch control moves both
// directions to the new epoch and tells each Experience; envelopes of the old epoch are then
// dropped and counted while their sequence numbers stay spent.
func TestEpochChangeKeepsTheClientPart(t *testing.T) {
	f := newConn(t, nil)
	var told []string
	f.s.OnEpoch(func(player uuid.UUID, exp string) {
		if player == f.player {
			told = append(told, exp)
		}
	})
	accept := f.activate()
	if err := f.conn.WritePacket(&packet.ChangeDimension{Dimension: 1}); err != nil {
		t.Fatal(err)
	}
	if !f.sendToClient(1) {
		t.Fatal("a dimension change ended a v2 client part")
	}
	f.deliver(f.envelope(accept, 1, nil))
	next := uint64(testEpoch + 1000)
	f.deliver(f.encode(Control{Epoch: &Epoch{accept.Session, next}}))
	if !slices.Equal(told, []string{f.manifest.ID}) {
		t.Fatalf("told %q of the epoch", told)
	}
	if !f.sendToClient(2) {
		t.Fatal("send refused after the epoch")
	}
	var e Envelope
	if out := f.carrier(); Decode(out[len(out)-1], &e) != nil || e.WorldEpoch != next || e.Sequence != 2 {
		t.Fatalf("envelope %s; want sequence 2 in the new epoch", out[len(out)-1])
	}
	f.deliver(
		f.envelope(accept, 2, nil),
		f.envelope(accept, 3, func(e *Envelope) { e.WorldEpoch = next }),
	)
	if n := len(f.messages()); n != 2 {
		t.Fatalf("%d messages delivered, want the old epoch's first and the new epoch's", n)
	}
	if stale := f.conn.(*Conn).ingress.Stale; stale != 1 {
		t.Fatalf("%d stale envelopes counted, want 1", stale)
	}
	if !f.sendToClient(3) {
		t.Fatal("the old epoch's envelope ended the client part")
	}
	f.connected()
}

// An epoch control that names another session, does not move the epoch forward, comes before
// Ready or inside a fragmented message ends the client part.
func TestEpochViolationsFallBack(t *testing.T) {
	for _, c := range []struct {
		name     string
		ready    bool
		messages func(*connFixture, Accept) [][]byte
	}{
		{"another session", true, func(f *connFixture, a Accept) [][]byte {
			return [][]byte{f.encode(Control{Epoch: &Epoch{strings.Repeat("00", 32), testEpoch + 1}})}
		}},
		{"the same epoch", true, func(f *connFixture, a Accept) [][]byte {
			return [][]byte{f.encode(Control{Epoch: &Epoch{a.Session, testEpoch}})}
		}},
		{"an earlier epoch", true, func(f *connFixture, a Accept) [][]byte {
			return [][]byte{f.encode(Control{Epoch: &Epoch{a.Session, testEpoch - 1}})}
		}},
		{"before Ready", false, func(f *connFixture, a Accept) [][]byte {
			return [][]byte{f.encode(Control{Epoch: &Epoch{a.Session, testEpoch + 1}})}
		}},
		{"inside a fragmented message", true, func(f *connFixture, a Accept) [][]byte {
			var e Envelope
			if err := Decode(f.envelope(a, 1, nil), &e); err != nil {
				f.t.Fatal(err)
			}
			open := f.encode(Fragment{Header: e, Part: Part{Index: 0, Count: 2, Data: "[{"}})
			return [][]byte{open, f.encode(Control{Epoch: &Epoch{a.Session, testEpoch + 1}})}
		}},
	} {
		t.Run(c.name, func(t *testing.T) {
			f := newConn(t, nil)
			told := 0
			f.s.OnEpoch(func(uuid.UUID, string) { told++ })
			accept := f.accept(f.hello())
			if c.ready {
				r := f.ready(accept)
				f.deliver(f.encode(Control{Ready: &r}))
			}
			f.deliver(c.messages(f, accept)...)
			if told != 0 {
				t.Fatal("an Experience was told of a refused epoch")
			}
			f.fellBack()
		})
	}
}

// A record over the inline limit reaches the client in fragments its ingress reassembles; one
// over the per-message cap or the client's rate is not sent at all and spends no sequence number;
// a fragmented client message is reassembled before its Experience gets it.
func TestFragmentedMessagesBothWays(t *testing.T) {
	f := newConn(t, nil)
	accept := f.activate()
	ch := f.itemsChannel()
	payload := itemsRecord(150)
	if !f.s.Send(f.player, f.manifest.ID, ch.ID, ch.Schema, payload) {
		t.Fatal("the fragmented list was not sent")
	}
	out := f.carrier()[1:]
	if len(out) < 2 {
		t.Fatalf("%d carrier messages, want fragments", len(out))
	}
	whole, err := receiveAll(NewIngress(ToClient, 0), f.clientGrant(accept), testEpoch, out)
	if err != nil || len(whole) != 1 || !reflect.DeepEqual(whole[0].Payload, payload) {
		t.Fatalf("the client reassembled %d messages: %v", len(whole), err)
	}
	sent := len(f.carrier())
	if f.s.Send(f.player, f.manifest.ID, ch.ID, ch.Schema, itemsRecord(400)) {
		t.Fatal("sent a list over the per-message cap")
	}
	if f.s.Send(f.player, f.manifest.ID, ch.ID, ch.Schema, payload) {
		t.Fatal("sent a list over the client's remaining byte rate")
	}
	if len(f.carrier()) != sent {
		t.Fatal("a refused message sent some of its fragments")
	}
	f.clock.Advance(time.Second)
	if !f.sendToClient(1) {
		t.Fatal("refused after the rate recovered")
	}
	var e Envelope
	if last := f.carrier()[len(f.carrier())-1]; Decode(last, &e) != nil || e.Sequence != 2 {
		t.Fatalf("envelope %s; want sequence 2", last)
	}

	var ack Envelope
	if err := Decode(f.envelope(accept, 1, nil), &ack); err != nil {
		t.Fatal(err)
	}
	data := string(f.encode(Record(ack.Payload)))
	f.deliver(
		f.encode(Fragment{Header: ack, Part: Part{Index: 0, Count: 2, Data: data[:5]}}),
		f.encode(Fragment{Header: ack, Part: Part{Index: 1, Count: 2, Data: data[5:]}}),
	)
	if got := f.messages(); len(got) != 1 || got[0].payload != describe(ack.Payload) {
		t.Fatalf("received %+v", got)
	}
	f.connected()
}
