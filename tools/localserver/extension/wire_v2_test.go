package extension

import (
	"bytes"
	"reflect"
	"strings"
	"testing"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/experience"
)

// listOf and recordOf hold values as a decoder does: an empty one is an empty slice, not nil.
func listOf(values ...experience.Scalar) experience.Scalar {
	values = append([]experience.Scalar{}, values...)
	return experience.Scalar{List: &values}
}

func recordOf(values ...experience.Scalar) experience.Scalar {
	values = append([]experience.Scalar{}, values...)
	return experience.Scalar{Record: &values}
}

// goldenGrantV2 is the fixture session on the v2 Accept's wire.
func goldenGrantV2(t *testing.T) (*Grant, uint64) {
	t.Helper()
	grant, epoch := goldenGrant(t)
	var accept Accept
	decodeGolden(t, "accept_v2_payload.json", &accept)
	if accept.Wire == nil {
		t.Fatal("the v2 Accept selects no wire")
	}
	grant.Wire = *accept.Wire
	return grant, epoch
}

// goldenFragments are the carrier messages of the fixture's fragmented list, as sent.
func goldenFragments(t *testing.T) [][]byte {
	t.Helper()
	var parts fragments
	decodeGolden(t, "fragments_v2_list_record.json", &parts)
	out := make([][]byte, len(parts))
	for i, part := range parts {
		out[i] = mustEncode(t, part)
	}
	return out
}

func mustEncode(t *testing.T, m Message) []byte {
	t.Helper()
	data, err := Encode(m)
	if err != nil {
		t.Fatal(err)
	}
	return data
}

// receiveAll feeds messages to in a second apart, so only the rule under test refuses one.
func receiveAll(in *Ingress, grant *Grant, epoch uint64, messages [][]byte) ([]*Envelope, error) {
	var whole []*Envelope
	for i, data := range messages {
		envelope, err := in.Receive(data, uint64(i+1)*1000, epoch, grant)
		if err != nil {
			return whole, err
		}
		if envelope != nil {
			whole = append(whole, envelope)
		}
	}
	return whole, nil
}

// Rust's inline list and its fragmented list pass Go's ingress, and Go fragments the reassembled
// list into exactly Rust's carrier messages, cut inside no character.
func TestGoldenFragmentsMatchRust(t *testing.T) {
	grant, epoch := goldenGrantV2(t)
	inline := fixture(t, "envelope_v2_list_record.json")
	parts := goldenFragments(t)
	got, err := receiveAll(NewIngress(ToClient, 0), grant, epoch, append([][]byte{inline}, parts...))
	if err != nil || len(got) != 2 {
		t.Fatalf("received %d messages: %v", len(got), err)
	}
	if !bytes.Equal(mustEncode(t, got[0]), inline) {
		t.Fatalf("the inline list re-encodes as %s", mustEncode(t, got[0]))
	}
	messages, err := EncodeEnvelope(*got[1], grant.Wire)
	if err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(messages, parts) {
		t.Fatalf("Go fragments the list into %d messages unlike Rust's %d", len(messages), len(parts))
	}
	if inline, err := EncodeEnvelope(*got[0], grant.Wire); err != nil || len(inline) != 1 {
		t.Fatalf("the inline list encodes as %d messages: %v", len(inline), err)
	}
	if _, err := EncodeEnvelope(*got[1], v1Wire); err == nil {
		t.Fatal("v1 carried a list over its payload limit")
	}
}

// Lists and records nest at most MaxFieldDepth containers deep, records hold at most
// MaxChannelFields fields, and every nested value is checked, as Rust's Channel::validate does.
func TestListAndRecordValidationMirrorsClient(t *testing.T) {
	var channel Channel
	decodeGolden(t, "channel_list_record.json", &channel)
	if !channel.Declared() {
		t.Fatal("the fixture channel is not declared")
	}
	var envelope Envelope
	decodeGolden(t, "envelope_v2_list_record.json", &envelope)
	if err := channel.Validate(envelope.Payload, ToClient, MaxPayloadBytes); err != nil {
		t.Fatalf("the fixture record: %v", err)
	}
	item := func(id string, count int64, name string, flags ...experience.Scalar) experience.Scalar {
		return recordOf(text(id), integer(count), text(name), listOf(flags...))
	}
	flag := recordOf(boolean(true), choice(3))
	for _, c := range []struct {
		name    string
		payload Record
		ok      bool
	}{
		{"an empty list", Record{listOf(), recordOf()}, true},
		{"nested values at their bounds", Record{listOf(item("a", 0, "", flag, flag)), recordOf()}, true},
		{"a choice out of range", Record{listOf(item("a", 0, "", recordOf(boolean(true), choice(4)))), recordOf()}, false},
		{"a negative count", Record{listOf(item("a", -1, "")), recordOf()}, false},
		{"a record of the wrong arity", Record{listOf(recordOf(text("a"))), recordOf()}, false},
		{"a list for a record", Record{listOf(item("a", 0, "")), listOf()}, false},
		{"a record for a list", Record{recordOf(), recordOf()}, false},
		{"an id over its max_bytes", Record{listOf(item(strings.Repeat("a", 97), 0, "")), recordOf()}, false},
		{"flags over their max_items", Record{listOf(item("a", 0, "", flag, flag, flag, flag, flag, flag, flag, flag, flag)), recordOf()}, false},
	} {
		if err := channel.Validate(c.payload, ToClient, MaxMessageBytes); (err == nil) != c.ok {
			t.Errorf("%s: %v, want valid %v", c.name, err, c.ok)
		}
	}

	var field Field = BoolField{}
	value := boolean(true)
	for level := 1; level <= MaxFieldDepth+1; level++ {
		if level%2 == 0 {
			field, value = RecordField{Fields: []Field{field}}, recordOf(value)
		} else {
			field, value = ListField{Item: field, MaxItems: 1}, listOf(value)
		}
		nested := Channel{ID: "a.nested", Schema: 1, Direction: ToClient, Fields: []Field{field}}
		allowed := level <= MaxFieldDepth
		if nested.Declared() != allowed || (nested.Validate(Record{value}, ToClient, MaxMessageBytes) == nil) != allowed {
			t.Errorf("level %d: declared %v, want %v", level, nested.Declared(), allowed)
		}
	}
	wide := RecordField{}
	for range MaxChannelFields + 1 {
		wide.Fields = append(wide.Fields, BoolField{})
	}
	if (Channel{ID: "a.wide", Schema: 1, Direction: ToClient, Fields: []Field{wide}}).Declared() {
		t.Error("a record of more than MaxChannelFields fields is declared")
	}
}

// Every malformed, interleaved or over-budget fragment quarantines the ingress, as Rust's does.
func TestFragmentViolationsQuarantine(t *testing.T) {
	grant, epoch := goldenGrantV2(t)
	parts := goldenFragments(t)
	inline := fixture(t, "envelope_v2_list_record.json")
	var first Fragment
	if err := Decode(parts[0], &first); err != nil {
		t.Fatal(err)
	}
	// The fixture's fragments follow its inline envelope: sequence 2 after 1.
	changed := func(i int, change func(*Fragment)) []byte {
		var f Fragment
		if err := Decode(parts[i], &f); err != nil {
			t.Fatal(err)
		}
		change(&f)
		return mustEncode(t, f)
	}
	for _, c := range []struct {
		name     string
		messages [][]byte
	}{
		{"an envelope inside a message", [][]byte{inline, parts[0], inline}},
		{"a missing middle fragment", [][]byte{inline, parts[0], parts[2]}},
		{"a repeated fragment", [][]byte{inline, parts[0], parts[0]}},
		{"a later fragment first", [][]byte{inline, parts[1]}},
		{"a changed count", [][]byte{inline, parts[0], changed(1, func(f *Fragment) { f.Part.Count++ })}},
		{"a changed schema", [][]byte{inline, parts[0], changed(1, func(f *Fragment) { f.Header.Schema++ })}},
		{"a changed epoch", [][]byte{inline, parts[0], changed(1, func(f *Fragment) { f.Header.WorldEpoch++ })}},
		{"a one-fragment message", [][]byte{inline, changed(0, func(f *Fragment) { f.Part.Count = 1 })}},
		{"empty data", [][]byte{inline, changed(0, func(f *Fragment) { f.Part.Data = "" })}},
		{"data over the fragment limit", [][]byte{inline, changed(0, func(f *Fragment) { f.Part.Data += strings.Repeat("x", MaxPayloadBytes) })}},
		{"a sequence gap", [][]byte{parts[0]}},
		{"a fragment on wire v1", [][]byte{inline, changed(0, func(f *Fragment) { f.Header.Version = WireVersion })}},
		{"data that is not a record", [][]byte{inline, changed(0, func(f *Fragment) { f.Part.Count = 2 }), changed(1, func(f *Fragment) { f.Part.Count = 2 })}},
		{"an overflowing message", func() [][]byte {
			messages := [][]byte{inline}
			for i := range MaxMessageBytes/MaxPayloadBytes + 1 {
				messages = append(messages, changed(0, func(f *Fragment) {
					f.Part.Index, f.Part.Count, f.Part.Data = uint32(i), 1<<20, strings.Repeat("x", MaxPayloadBytes)
				}))
			}
			return messages
		}()},
	} {
		t.Run(c.name, func(t *testing.T) {
			in := NewIngress(ToClient, 0)
			if _, err := receiveAll(in, grant, epoch, c.messages); err == nil {
				t.Fatal("received")
			}
			if _, err := in.Receive(inline, 1<<40, epoch, grant); err == nil || !strings.Contains(err.Error(), "quarantined") {
				t.Fatalf("after the violation: %v; want the ingress quarantined", err)
			}
		})
	}

	// The open message counts against the per-connection budget, to the byte.
	budget := 0
	for _, part := range parts {
		budget += len(part)
	}
	for _, c := range []struct {
		budget int
		ok     bool
	}{{budget, true}, {budget - 1, false}} {
		tight := *grant
		tight.Wire.Limits.MaxReassemblyBytes = uint32(c.budget)
		if _, err := receiveAll(NewIngress(ToClient, 0), &tight, epoch, append([][]byte{inline}, parts...)); (err == nil) != c.ok {
			t.Errorf("budget %d: %v, want accepted %v", c.budget, err, c.ok)
		}
	}
	if first.Part.Index != 0 || len(first.Part.Data) >= MaxPayloadBytes {
		t.Error("the fixture's first cut does not fall inside a character")
	}
}

// ChargeAll spends credit for every message or for none.
func TestChargeAllIsAllOrNothing(t *testing.T) {
	rate := NewRateLimit(0)
	half := MaxBytesPerSecond / 2
	if err := rate.ChargeAll([]int{half, half + 1}, 0); err == nil {
		t.Fatal("charged one byte over a second's credit")
	}
	if err := rate.ChargeAll([]int{half, half}, 0); err != nil {
		t.Fatalf("a failed charge spent credit: %v", err)
	}
	many := make([]int, MaxMessagesPerSecond+1)
	fresh := NewRateLimit(0)
	if err := fresh.ChargeAll(many, 0); err == nil {
		t.Fatal("charged more messages than a second's credit")
	}
}

// Optional members decode from null or absence and encode only when present, as serde's
// skip_serializing_if leaves them out.
func TestOptionalMembersEncodeOnlyWhenSet(t *testing.T) {
	v1 := fixture(t, "hello_payload.json")
	withNull := append(bytes.TrimSuffix(bytes.Clone(v1), []byte("}")), []byte(`,"wire":null}`)...)
	var h Hello
	if err := Decode(withNull, &h); err != nil || h.Wire != nil {
		t.Fatalf("a null wire: %+v, %v", h.Wire, err)
	}
	if got := mustEncode(t, h); !bytes.Equal(got, v1) {
		t.Fatalf("re-encoded as %s", got)
	}
	var m Manifest
	decodeGolden(t, "manifest_payload.json", &m)
	m.Templates = []string{"ui/b.json", "ui/a.json"}
	encoded := string(mustEncode(t, m))
	if !strings.Contains(encoded, `,"templates":["ui/a.json","ui/b.json"],"files":`) {
		t.Fatalf("templates encode as %s", encoded)
	}
}
