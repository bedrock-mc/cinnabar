package extension

import (
	"strings"
	"testing"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/experience"
)

func boolean(v bool) experience.Scalar  { return experience.Scalar{Bool: &v} }
func integer(v int64) experience.Scalar { return experience.Scalar{Integer: &v} }
func text(v string) experience.Scalar   { return experience.Scalar{Text: &v} }
func choice(v uint16) experience.Scalar { return experience.Scalar{Choice: &v} }

// allFields is the fixture channel that declares every field type, with its fixture record.
func allFields(t *testing.T) (Channel, Record) {
	t.Helper()
	var channel Channel
	decodeGolden(t, "channel_all_field_types.json", &channel)
	var envelope Envelope
	decodeGolden(t, "envelope_all_scalar_types.json", &envelope)
	return channel, envelope.Payload
}

// Channel.Validate is Rust's: the declared direction, the declared field count, each value of its
// declared type and range, and at most MaxPayloadBytes of encoded record.
func TestChannelValidateMirrorsClient(t *testing.T) {
	channel, record := allFields(t)
	if err := channel.Validate(record, ToClient, MaxPayloadBytes); err != nil {
		t.Fatalf("the fixture record: %v", err)
	}
	if err := channel.Validate(record, ToServer, MaxPayloadBytes); err == nil {
		t.Error("a to_client channel validated a to_server record")
	}
	if err := channel.Validate(record[:len(record)-1], ToClient, MaxPayloadBytes); err == nil {
		t.Error("a record with a field missing validated")
	}
	if err := channel.Validate(append(record[:len(record):len(record)], boolean(true)), ToClient, MaxPayloadBytes); err == nil {
		t.Error("a record with an extra field validated")
	}
	invalid := channel
	invalid.ID = "Benergistics.all_fields"
	if err := invalid.Validate(record, ToClient, MaxPayloadBytes); err == nil {
		t.Error("a channel whose id is no identifier validated")
	}

	bounded := Channel{ID: "a.bounded", Schema: 1, Direction: ToServer, Fields: []Field{
		BoolField{}, IntegerField{Min: -2, Max: 5}, TextField{MaxBytes: 3}, ChoiceField{Variants: 2},
	}}
	valid := Record{boolean(false), integer(-2), text("é."), choice(1)}
	if err := bounded.Validate(valid, ToServer, MaxPayloadBytes); err != nil {
		t.Fatalf("a record at every bound: %v", err)
	}
	for _, c := range []struct {
		name  string
		at    int
		value experience.Scalar
		ok    bool
	}{
		{"integer inside", 1, integer(5), true},
		{"integer below min", 1, integer(-3), false},
		{"integer above max", 1, integer(6), false},
		{"text over max_bytes in UTF-8", 2, text("éé"), false},
		{"text over max_bytes", 2, text("abcd"), false},
		{"choice at variants", 3, choice(2), false},
		{"bool for an integer", 1, boolean(true), false},
		{"integer for a bool", 0, integer(0), false},
		{"empty scalar", 0, experience.Scalar{}, false},
	} {
		changed := append(Record(nil), valid...)
		changed[c.at] = c.value
		if err := bounded.Validate(changed, ToServer, MaxPayloadBytes); (err == nil) != c.ok {
			t.Errorf("%s: %v, want valid %v", c.name, err, c.ok)
		}
	}
}

// The field count limit is Rust's MAX_CHANNEL_FIELDS: exactly that many pass, one more does not.
func TestChannelFieldLimit(t *testing.T) {
	channel := Channel{ID: "a.many", Schema: 1, Direction: ToClient}
	var record Record
	for range MaxChannelFields {
		channel.Fields = append(channel.Fields, BoolField{})
		record = append(record, boolean(true))
	}
	if err := channel.Validate(record, ToClient, MaxPayloadBytes); err != nil {
		t.Fatalf("%d fields: %v", MaxChannelFields, err)
	}
	channel.Fields = append(channel.Fields, BoolField{})
	record = append(record, boolean(true))
	if err := channel.Validate(record, ToClient, MaxPayloadBytes); err == nil {
		t.Fatalf("%d fields validated", MaxChannelFields+1)
	}
}

// The payload limit counts the encoded record, escapes included, like Rust's
// serde_json::to_vec(payload).len().
func TestChannelPayloadLimit(t *testing.T) {
	channel := Channel{ID: "a.text", Schema: 1, Direction: ToClient, Fields: []Field{TextField{MaxBytes: 65535}}}
	empty, err := Encode(Record{text("")})
	if err != nil {
		t.Fatal(err)
	}
	room := MaxPayloadBytes - len(empty)
	if err := channel.Validate(Record{text(strings.Repeat("a", room))}, ToClient, MaxPayloadBytes); err != nil {
		t.Fatalf("a record of exactly MaxPayloadBytes: %v", err)
	}
	if err := channel.Validate(Record{text(strings.Repeat("a", room+1))}, ToClient, MaxPayloadBytes); err == nil {
		t.Fatal("a record one byte over MaxPayloadBytes validated")
	}
	if err := channel.Validate(Record{text(strings.Repeat(`"`, room/2+1))}, ToClient, MaxPayloadBytes); err == nil {
		t.Fatal("a record whose escapes exceed MaxPayloadBytes validated")
	}
}

// RateLimit is Rust's: one second of burst, refill per millisecond, no credit from a clock that
// goes back and no envelope over MaxEnvelopeBytes.
func TestRateLimitMirrorsClient(t *testing.T) {
	rate := NewRateLimit(1000)
	for range MaxMessagesPerSecond {
		if err := rate.Charge(1, 1000); err != nil {
			t.Fatal(err)
		}
	}
	if err := rate.Charge(1, 999); err == nil {
		t.Fatal("a reversed clock minted credit")
	}
	if err := rate.Charge(1, 1015); err == nil {
		t.Fatal("15 ms refilled a whole message")
	}
	if err := rate.Charge(1, 1016); err != nil {
		t.Fatalf("16 ms refilled less than a message: %v", err)
	}
	if err := rate.Charge(1, 2000); err != nil {
		t.Fatal(err)
	}
	if err := rate.Charge(MaxBytesPerSecond, 2000); err == nil {
		t.Fatal("a second of bytes passed after bytes were spent")
	}

	idle := NewRateLimit(0)
	for i := range MaxMessagesPerSecond + 1 {
		err := idle.Charge(1, 100_000)
		if (err != nil) != (i == MaxMessagesPerSecond) {
			t.Fatalf("message %d after a long idle: %v", i, err)
		}
	}

	fresh := NewRateLimit(0)
	if err := fresh.Charge(MaxEnvelopeBytes+1, 0); err == nil || !strings.Contains(err.Error(), "too large") {
		t.Fatalf("an envelope over MaxEnvelopeBytes: %v", err)
	}
	if err := fresh.Charge(MaxEnvelopeBytes, 0); err != nil {
		t.Fatalf("an envelope of exactly MaxEnvelopeBytes: %v", err)
	}
}

// goldenGrant is the fixture session: the offer, Accept's route, Ready's permissions and epoch and
// the manifest's channels.
func goldenGrant(t *testing.T) (*Grant, uint64) {
	t.Helper()
	var offer Offer
	decodeGolden(t, "offer_payload.json", &offer)
	var accept Accept
	decodeGolden(t, "accept_payload.json", &accept)
	var manifest Manifest
	decodeGolden(t, "manifest_payload.json", &manifest)
	var ready Control
	decodeGolden(t, "ready_message.json", &ready)
	return &Grant{
		Offer:      &offer,
		Session:    accept.Session,
		Connection: accept.Hello.Connection,
		Subclient:  accept.Hello.Subclient,
		Wire:       v1Wire,
		Recipients: map[string]Recipient{manifest.ID: {
			Permissions: ready.Ready.Permissions[manifest.ID],
			Channels:    manifest.Channels,
		}},
	}, ready.Ready.WorldEpoch
}

// encodeEnvelope encodes a changed copy of the to_server fixture.
func encodeEnvelope(t *testing.T, change func(*Envelope)) []byte {
	t.Helper()
	var envelope Envelope
	decodeGolden(t, "envelope_to_server.json", &envelope)
	change(&envelope)
	data, err := Encode(envelope)
	if err != nil {
		t.Fatal(err)
	}
	return data
}

// The fixture envelopes pass the ingress of their direction, and a replay quarantines it.
func TestIngressAcceptsFixtureEnvelopes(t *testing.T) {
	grant, epoch := goldenGrant(t)
	for _, c := range []struct {
		name      string
		direction Direction
	}{{"envelope_to_server.json", ToServer}, {"envelope_to_client.json", ToClient}} {
		t.Run(c.name, func(t *testing.T) {
			data := fixture(t, c.name)
			ingress := NewIngress(c.direction, 0)
			envelope, err := ingress.Receive(data, 0, epoch, grant)
			if err != nil || envelope == nil {
				t.Fatalf("Receive: %v, %v", envelope, err)
			}
			if got, _ := Encode(envelope); string(got) != string(data) {
				t.Fatalf("received %s", got)
			}
			if _, err := ingress.Receive(data, 0, epoch, grant); err == nil {
				t.Fatal("a replay was received")
			}
			next := encodeEnvelope(t, func(e *Envelope) { e.Sequence = 2 })
			if _, err := ingress.Receive(next, 0, epoch, grant); err == nil || !strings.Contains(err.Error(), "quarantined") {
				t.Fatalf("after a replay: %v; want the ingress quarantined", err)
			}
		})
	}
}

// Every rule of Rust's Ingress::receive refuses the envelope that breaks it.
func TestIngressRejects(t *testing.T) {
	for _, c := range []struct {
		name   string
		change func(*Envelope)
		grant  func(*Grant)
		data   []byte
	}{
		{name: "version", change: func(e *Envelope) { e.Version = 2 }},
		{name: "session", change: func(e *Envelope) { e.Session = strings.Repeat("0", 64) }},
		{name: "connection", change: func(e *Envelope) { e.Connection = strings.Repeat("0", 64) }},
		{name: "subclient", change: func(e *Envelope) { e.Subclient = 1 }},
		{name: "sequence gap", change: func(e *Envelope) { e.Sequence = 2 }},
		{name: "sequence zero", change: func(e *Envelope) { e.Sequence = 0 }},
		{name: "generation", change: func(e *Envelope) { e.Generation = 2 }},
		{name: "bundle outside the offer", change: func(e *Envelope) { e.Bundle = "other"; e.Channel = "other.ack" }},
		{name: "foreign namespace", change: func(e *Envelope) { e.Channel = "benergisticsx.ack" }},
		{name: "wrong direction", change: func(e *Envelope) { e.Channel = "benergistics.controller" }},
		{name: "out of range", change: func(e *Envelope) { e.Payload = Record{integer(1 << 32)} }},
		{name: "wrong type", change: func(e *Envelope) { e.Payload = Record{boolean(true)} }},
		{name: "offer without messaging", grant: func(g *Grant) { g.Offer.Scope.Permissions = NewPermissions(PermissionUI) }},
		{name: "recipient without messaging", grant: func(g *Grant) {
			recipient := g.Recipients["benergistics"]
			recipient.Permissions = NewPermissions(PermissionUI)
			g.Recipients["benergistics"] = recipient
		}},
		{name: "no recipient", grant: func(g *Grant) { delete(g.Recipients, "benergistics") }},
		{name: "unknown member", data: []byte(strings.Replace(string(fixture(t, "envelope_to_server.json")), `"version":1`, `"version":1,"extra":1`, 1))},
		{name: "malformed", data: []byte(`{"version":`)},
		{name: "oversized", data: []byte(strings.Repeat(" ", MaxEnvelopeBytes+1))},
	} {
		t.Run(c.name, func(t *testing.T) {
			grant, epoch := goldenGrant(t)
			if c.grant != nil {
				c.grant(grant)
			}
			data := c.data
			if data == nil {
				data = encodeEnvelope(t, func(e *Envelope) {
					if c.change != nil {
						c.change(e)
					}
				})
			}
			if envelope, err := NewIngress(ToServer, 0).Receive(data, 0, epoch, grant); err == nil {
				t.Fatalf("received %+v", envelope)
			}
		})
	}
}

// An unknown (channel, schema) and an envelope of another world epoch are skipped and counted
// apart but consume their sequence number, as Rust's ingress and peek do.
func TestIngressSkipsUnknownSchemaAndStaleEpoch(t *testing.T) {
	grant, epoch := goldenGrant(t)
	ingress := NewIngress(ToServer, 0)
	for sequence, change := range []func(*Envelope){
		func(e *Envelope) { e.Schema = 2 },
		func(e *Envelope) { e.Channel = "benergistics.unknown" },
		func(e *Envelope) { e.WorldEpoch = epoch + 1 },
	} {
		data := encodeEnvelope(t, func(e *Envelope) {
			e.Sequence = uint64(sequence) + 1
			change(e)
		})
		envelope, err := ingress.Receive(data, 0, epoch, grant)
		if err != nil || envelope != nil {
			t.Fatalf("sequence %d: %v, %v; want it skipped", sequence+1, envelope, err)
		}
	}
	if ingress.Skipped != 2 || ingress.Stale != 1 {
		t.Fatalf("skipped %d and stale %d, want 2 and 1", ingress.Skipped, ingress.Stale)
	}
	data := encodeEnvelope(t, func(e *Envelope) { e.Sequence = 4 })
	if envelope, err := ingress.Receive(data, 0, epoch, grant); err != nil || envelope == nil {
		t.Fatalf("sequence 4 after three skips: %v", err)
	}
}

// Ingress charges its rate limit before parsing: MaxMessagesPerSecond valid envelopes pass in one
// millisecond and the next does not.
func TestIngressChargesRate(t *testing.T) {
	grant, epoch := goldenGrant(t)
	ingress := NewIngress(ToServer, 0)
	for sequence := uint64(1); sequence <= MaxMessagesPerSecond+1; sequence++ {
		data := encodeEnvelope(t, func(e *Envelope) { e.Sequence = sequence })
		_, err := ingress.Receive(data, 0, epoch, grant)
		if (err != nil) != (sequence == MaxMessagesPerSecond+1) {
			t.Fatalf("envelope %d: %v", sequence, err)
		}
	}
}
