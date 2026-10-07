package extension

import (
	"errors"
	"fmt"
	"slices"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/experience"
)

// appendScalar appends s as Rust serializes a wire::Scalar: {"type":…,"value":…}, a list or
// record holding its values in order. A scalar with no field or more than one set has no Rust
// value.
func appendScalar(b []byte, s experience.Scalar) ([]byte, error) {
	set := 0
	for _, field := range []bool{s.Bool != nil, s.Integer != nil, s.Text != nil, s.Choice != nil, s.List != nil, s.Record != nil} {
		if field {
			set++
		}
	}
	if set != 1 {
		return nil, fmt.Errorf("a scalar holds %d values, not 1", set)
	}
	switch {
	case s.Bool != nil:
		b = append(b, `{"type":"bool","value":`...)
		b = strconv.AppendBool(b, *s.Bool)
	case s.Integer != nil:
		b = append(b, `{"type":"integer","value":`...)
		b = strconv.AppendInt(b, *s.Integer, 10)
	case s.Text != nil:
		b = append(b, `{"type":"text","value":`...)
		var err error
		if b, err = appendString(b, *s.Text); err != nil {
			return nil, err
		}
	case s.Choice != nil:
		b = append(b, `{"type":"choice","value":`...)
		b = strconv.AppendUint(b, uint64(*s.Choice), 10)
	case s.List != nil, s.Record != nil:
		values, kind := s.List, "list"
		if s.Record != nil {
			values, kind = s.Record, "record"
		}
		b = append(b, `{"type":"`...)
		b = append(b, kind...)
		b = append(b, `","value":`...)
		var err error
		if b, err = Record(*values).appendJSON(b); err != nil {
			return nil, err
		}
	}
	return append(b, '}'), nil
}

var scalarFields = []string{"type", "value"}

// readScalar reads a Rust wire::Scalar: adjacently tagged, either member first, value required.
func readScalar(r *reader) (experience.Scalar, error) {
	kind, raw, err := r.tagged("type")
	if err != nil {
		return experience.Scalar{}, err
	}
	var s experience.Scalar
	var value func(*reader) error
	values := func(p **[]experience.Scalar) func(*reader) error {
		return func(r *reader) error {
			var rec Record
			err := rec.decodeJSON(r)
			if rec == nil {
				rec = Record{}
			}
			list := []experience.Scalar(rec)
			*p = &list
			return err
		}
	}
	switch kind {
	case "bool":
		value = func(r *reader) error { v, err := r.boolean(); s.Bool = &v; return err }
	case "integer":
		value = func(r *reader) error { v, err := r.int64(); s.Integer = &v; return err }
	case "text":
		value = func(r *reader) error { v, err := r.str(); s.Text = &v; return err }
	case "choice":
		value = func(r *reader) error { v, err := readUint[uint16](r); s.Choice = &v; return err }
	case "list":
		value = values(&s.List)
	case "record":
		value = values(&s.Record)
	default:
		return experience.Scalar{}, r.errorf("unknown scalar type %q", kind)
	}
	err = within(raw, func(sub *reader) error {
		// The sub-reader starts at the depth of the object it reads, so nesting stays bounded
		// by serde_json's recursion limit.
		sub.depth = r.depth
		return sub.fields(scalarFields, func(name string) error {
			if name == "type" {
				_, err := sub.str()
				return err
			}
			return value(sub)
		})
	})
	if err != nil {
		return experience.Scalar{}, err
	}
	return s, nil
}

// Record is a Rust Vec<wire::Scalar>: an envelope's payload, which the client part's guest sends
// and receives as record-json.
type Record []experience.Scalar

func (rec Record) appendJSON(b []byte) ([]byte, error) {
	b = append(b, '[')
	for i, s := range rec {
		if i > 0 {
			b = append(b, ',')
		}
		var err error
		if b, err = appendScalar(b, s); err != nil {
			return nil, err
		}
	}
	return append(b, ']'), nil
}

func (rec *Record) decodeJSON(r *reader) error {
	*rec = nil
	return r.array(func() error {
		s, err := readScalar(r)
		*rec = append(*rec, s)
		return err
	})
}

// Field is a Rust wire::Field, the declared type of one record position: BoolField,
// IntegerField, TextField, ChoiceField, ListField or RecordField.
type Field interface {
	field()
}

// BoolField declares a bool.
type BoolField struct{}

// IntegerField declares an integer in Min..=Max.
type IntegerField struct{ Min, Max int64 }

// TextField declares a string of at most MaxBytes UTF-8 bytes.
type TextField struct{ MaxBytes uint16 }

// ChoiceField declares a choice in 0..Variants.
type ChoiceField struct{ Variants uint16 }

// ListField declares at most MaxItems values of type Item.
type ListField struct {
	Item     Field
	MaxItems uint16
}

// RecordField declares one value of each of Fields, in order.
type RecordField struct{ Fields []Field }

func (BoolField) field()    {}
func (IntegerField) field() {}
func (TextField) field()    {}
func (ChoiceField) field()  {}
func (ListField) field()    {}
func (RecordField) field()  {}

// fieldMembers are the serde names of each field type's members, tag first.
var fieldMembers = map[string][]string{
	"bool":    {"type"},
	"integer": {"type", "min", "max"},
	"text":    {"type", "max_bytes"},
	"choice":  {"type", "variants"},
	"list":    {"type", "item", "max_items"},
	"record":  {"type", "fields"},
}

// fieldList is a Rust Vec<wire::Field>.
type fieldList []Field

func (l fieldList) appendJSON(b []byte) ([]byte, error) {
	b = append(b, '[')
	for i, field := range l {
		if i > 0 {
			b = append(b, ',')
		}
		var err error
		if b, err = appendField(b, field); err != nil {
			return nil, fmt.Errorf("field %d: %w", i, err)
		}
	}
	return append(b, ']'), nil
}

// appendField appends a Rust wire::Field: internally tagged, its members after "type".
func appendField(b []byte, field Field) ([]byte, error) {
	switch f := field.(type) {
	case BoolField:
		b = append(b, `{"type":"bool"}`...)
	case IntegerField:
		b = append(b, `{"type":"integer","min":`...)
		b = strconv.AppendInt(b, f.Min, 10)
		b = append(b, `,"max":`...)
		b = strconv.AppendInt(b, f.Max, 10)
		b = append(b, '}')
	case TextField:
		b = append(b, `{"type":"text","max_bytes":`...)
		b = strconv.AppendUint(b, uint64(f.MaxBytes), 10)
		b = append(b, '}')
	case ChoiceField:
		b = append(b, `{"type":"choice","variants":`...)
		b = strconv.AppendUint(b, uint64(f.Variants), 10)
		b = append(b, '}')
	case ListField:
		b = append(b, `{"type":"list","item":`...)
		var err error
		if b, err = appendField(b, f.Item); err != nil {
			return nil, err
		}
		b = append(b, `,"max_items":`...)
		b = strconv.AppendUint(b, uint64(f.MaxItems), 10)
		b = append(b, '}')
	case RecordField:
		b = append(b, `{"type":"record","fields":`...)
		var err error
		if b, err = fieldList(f.Fields).appendJSON(b); err != nil {
			return nil, err
		}
		b = append(b, '}')
	default:
		return nil, errors.New("a field with no type")
	}
	return b, nil
}

// readField reads a Rust wire::Field: internally tagged, its members beside "type".
func readField(r *reader) (Field, error) {
	kind, raw, err := r.tagged("type")
	if err != nil {
		return nil, err
	}
	names, ok := fieldMembers[kind]
	if !ok {
		return nil, r.errorf("unknown field type %q", kind)
	}
	var integer IntegerField
	var text TextField
	var choice ChoiceField
	var list ListField
	var record RecordField
	err = within(raw, func(sub *reader) error {
		sub.depth = r.depth
		return sub.fields(names, func(name string) (err error) {
			switch name {
			case "type":
				_, err = sub.str()
			case "min":
				integer.Min, err = sub.int64()
			case "max":
				integer.Max, err = sub.int64()
			case "max_bytes":
				text.MaxBytes, err = readUint[uint16](sub)
			case "variants":
				choice.Variants, err = readUint[uint16](sub)
			case "item":
				list.Item, err = readField(sub)
			case "max_items":
				list.MaxItems, err = readUint[uint16](sub)
			case "fields":
				record.Fields, err = readFields(sub)
			}
			return err
		})
	})
	if err != nil {
		return nil, err
	}
	switch kind {
	case "integer":
		return integer, nil
	case "text":
		return text, nil
	case "choice":
		return choice, nil
	case "list":
		return list, nil
	case "record":
		return record, nil
	}
	return BoolField{}, nil
}

// readFields reads a Rust Vec<wire::Field>, never nil.
func readFields(r *reader) ([]Field, error) {
	fields := []Field{}
	err := r.array(func() error {
		field, err := readField(r)
		fields = append(fields, field)
		return err
	})
	return fields, err
}

// declared reports whether f is a declaration Rust's Field::declared accepts with depth more
// containers allowed: records of at most MaxChannelFields fields, nested at most that deep.
func declared(f Field, depth int) bool {
	switch f := f.(type) {
	case ListField:
		return depth > 0 && declared(f.Item, depth-1)
	case RecordField:
		return depth > 0 && len(f.Fields) <= MaxChannelFields &&
			!slices.ContainsFunc(f.Fields, func(field Field) bool { return !declared(field, depth-1) })
	case BoolField, IntegerField, TextField, ChoiceField:
		return true
	}
	return false
}

// admits reports whether value is of f's type and in its range, every nested value included, as
// Rust's Field::admits does.
func admits(f Field, value experience.Scalar) bool {
	switch f := f.(type) {
	case BoolField:
		return value.Bool != nil
	case IntegerField:
		return value.Integer != nil && f.Min <= *value.Integer && *value.Integer <= f.Max
	case TextField:
		return value.Text != nil && len(*value.Text) <= int(f.MaxBytes)
	case ChoiceField:
		return value.Choice != nil && *value.Choice < f.Variants
	case ListField:
		return value.List != nil && len(*value.List) <= int(f.MaxItems) &&
			!slices.ContainsFunc(*value.List, func(item experience.Scalar) bool { return !admits(f.Item, item) })
	case RecordField:
		if value.Record == nil || len(*value.Record) != len(f.Fields) {
			return false
		}
		for i, field := range f.Fields {
			if !admits(field, (*value.Record)[i]) {
				return false
			}
		}
		return true
	}
	return false
}

// Direction is a Rust wire::Direction.
type Direction uint8

// The directions, in Rust's declaration order.
const (
	ToClient Direction = iota
	ToServer
)

// directionNames are the serde names of the directions, indexed by Direction.
var directionNames = [...]string{"to_client", "to_server"}

func (d Direction) String() string {
	if int(d) < len(directionNames) {
		return directionNames[d]
	}
	return fmt.Sprintf("Direction(%d)", uint8(d))
}

func (d Direction) appendJSON(b []byte) ([]byte, error) {
	if int(d) >= len(directionNames) {
		return nil, fmt.Errorf("%v is no Rust direction", d)
	}
	return appendString(b, directionNames[d])
}

func (d *Direction) decodeJSON(r *reader) error {
	name, err := r.str()
	if err != nil {
		return err
	}
	i := slices.Index(directionNames[:], name)
	if i < 0 {
		return r.errorf("unknown direction %q", name)
	}
	*d = Direction(i)
	return nil
}

// Channel is a Rust wire::Channel, one typed channel that a manifest declares.
type Channel struct {
	ID        string
	Schema    uint16
	Direction Direction
	Fields    []Field
}

var channelFields = []string{"id", "schema", "direction", "fields"}

func (c Channel) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: channelFields}
	o.str(c.ID)
	o.uint(uint64(c.Schema))
	o.value(c.Direction)
	o.value(fieldList(c.Fields))
	return o.end()
}

func (c *Channel) decodeJSON(r *reader) error {
	*c = Channel{}
	return r.fields(channelFields, func(name string) (err error) {
		switch name {
		case "id":
			c.ID, err = r.str()
		case "schema":
			c.Schema, err = readUint[uint16](r)
		case "direction":
			err = c.Direction.decodeJSON(r)
		case "fields":
			c.Fields, err = readFields(r)
		}
		return err
	})
}

// Declared reports whether the declaration is one Rust's Channel::declared accepts: an
// identifier id, at most MaxChannelFields fields per record and lists and records nested at most
// MaxFieldDepth deep.
func (c Channel) Declared() bool {
	return Identifier(c.ID) && len(c.Fields) <= MaxChannelFields &&
		!slices.ContainsFunc(c.Fields, func(f Field) bool { return !declared(f, MaxFieldDepth) })
}

// Validate checks payload against the channel as Rust's Channel::validate does before guest
// dispatch or sending: a declaration Declared accepts, the declared direction, one value of the
// declared type and range per field, every nested value included, and at most maxBytes of
// encoded record.
func (c Channel) Validate(payload []experience.Scalar, direction Direction, maxBytes int) error {
	if !c.Declared() || c.Direction != direction {
		return errors.New("channel denied")
	}
	if len(payload) != len(c.Fields) {
		return errors.New("record field count mismatch")
	}
	for i, field := range c.Fields {
		if !admits(field, payload[i]) {
			return fmt.Errorf("record field %d rejected", i)
		}
	}
	encoded, err := Encode(Record(payload))
	if err != nil {
		return err
	}
	if len(encoded) > maxBytes {
		return errors.New("payload too large")
	}
	return nil
}

// Identifier reports whether text is an identifier as Rust's manifest::identifier defines one:
// 1 to MaxIdentifierBytes of lowercase ASCII letters, digits and ':', '_', '-', '.'.
func Identifier(text string) bool {
	if text == "" || len(text) > MaxIdentifierBytes {
		return false
	}
	for i := range len(text) {
		c := text[i]
		if !('a' <= c && c <= 'z' || '0' <= c && c <= '9' || strings.IndexByte(":_-.", c) >= 0) {
			return false
		}
	}
	return true
}

// Envelope is a Rust wire::Envelope, one typed channel message on the carrier.
type Envelope struct {
	Version    uint16
	Session    string
	Connection string
	Subclient  uint8
	Bundle     string
	Generation uint64
	Channel    string
	Schema     uint16
	Sequence   uint64
	WorldEpoch uint64
	Payload    []experience.Scalar
}

var envelopeFields = []string{"version", "session", "connection", "subclient", "bundle", "generation", "channel", "schema", "sequence", "world_epoch", "payload"}

// appendHeader appends every member of e but its payload, which a fragment replaces.
func (e Envelope) appendHeader(o *object) {
	o.uint(uint64(e.Version))
	o.str(e.Session)
	o.str(e.Connection)
	o.uint(uint64(e.Subclient))
	o.str(e.Bundle)
	o.uint(e.Generation)
	o.str(e.Channel)
	o.uint(uint64(e.Schema))
	o.uint(e.Sequence)
	o.uint(e.WorldEpoch)
}

func (e Envelope) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: envelopeFields}
	e.appendHeader(&o)
	o.value(Record(e.Payload))
	return o.end()
}

// decodeHeader reads the member name of e's header.
func (e *Envelope) decodeHeader(r *reader, name string) (err error) {
	switch name {
	case "version":
		e.Version, err = readUint[uint16](r)
	case "session":
		e.Session, err = r.str()
	case "connection":
		e.Connection, err = r.str()
	case "subclient":
		e.Subclient, err = readUint[uint8](r)
	case "bundle":
		e.Bundle, err = r.str()
	case "generation":
		e.Generation, err = readUint[uint64](r)
	case "channel":
		e.Channel, err = r.str()
	case "schema":
		e.Schema, err = readUint[uint16](r)
	case "sequence":
		e.Sequence, err = readUint[uint64](r)
	case "world_epoch":
		e.WorldEpoch, err = readUint[uint64](r)
	}
	return err
}

func (e *Envelope) decodeJSON(r *reader) error {
	*e = Envelope{}
	return r.fields(envelopeFields, func(name string) error {
		if name == "payload" {
			return (*Record)(&e.Payload).decodeJSON(r)
		}
		return e.decodeHeader(r, name)
	})
}

// Part is a Rust wire::Part: one ordered piece of a payload too large to send inline. Data is
// whole UTF-8 characters of the payload's JSON.
type Part struct {
	Index, Count uint32
	Data         string
}

var partFields = []string{"index", "count", "data"}

func (p Part) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: partFields}
	o.uint(uint64(p.Index))
	o.uint(uint64(p.Count))
	o.str(p.Data)
	return o.end()
}

func (p *Part) decodeJSON(r *reader) error {
	*p = Part{}
	return r.fields(partFields, func(name string) (err error) {
		switch name {
		case "index":
			p.Index, err = readUint[uint32](r)
		case "count":
			p.Count, err = readUint[uint32](r)
		case "data":
			p.Data, err = r.str()
		}
		return err
	})
}

// Fragment is a Rust wire::Fragment: a wire v2 envelope that carries one part of its payload
// instead of the payload. Header is the envelope's, its Payload unused; all parts of a message
// share it, sequence number included.
type Fragment struct {
	Header Envelope
	Part   Part
}

var fragmentFields = append(slices.Clone(envelopeFields[:len(envelopeFields)-1]), "fragment")

func (f Fragment) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: fragmentFields}
	f.Header.appendHeader(&o)
	o.value(f.Part)
	return o.end()
}

func (f *Fragment) decodeJSON(r *reader) error {
	*f = Fragment{}
	return r.fields(fragmentFields, func(name string) error {
		if name == "fragment" {
			return f.Part.decodeJSON(r)
		}
		return f.Header.decodeHeader(r, name)
	})
}

// sameHeader reports whether a and b are one message's header.
func sameHeader(a, b Envelope) bool {
	return a.Version == b.Version && a.Session == b.Session && a.Connection == b.Connection &&
		a.Subclient == b.Subclient && a.Bundle == b.Bundle && a.Generation == b.Generation &&
		a.Channel == b.Channel && a.Schema == b.Schema && a.Sequence == b.Sequence && a.WorldEpoch == b.WorldEpoch
}

// EncodeEnvelope returns the carrier messages of e under w, as Rust's wire::encode does: e itself
// while its payload fits inline, otherwise (on wire v2) fragments of the payload's JSON in order,
// each cut at the last character boundary within w's fragment limit.
func EncodeEnvelope(e Envelope, w Wire) ([][]byte, error) {
	if e.Version != w.Version {
		return nil, errors.New("envelope of another wire version")
	}
	payload, err := Encode(Record(e.Payload))
	if err != nil {
		return nil, err
	}
	limit := int(w.Limits.MaxFragmentBytes)
	if len(payload) <= limit {
		data, err := Encode(e)
		return [][]byte{data}, err
	}
	if w.Version == WireVersion || len(payload) > int(w.Limits.MaxMessageBytes) {
		return nil, errors.New("payload too large")
	}
	var parts []string
	for rest := string(payload); rest != ""; {
		end := min(limit, len(rest))
		for end > 0 && end < len(rest) && !utf8.RuneStart(rest[end]) {
			end--
		}
		if end == 0 {
			return nil, errors.New("fragment limit below one character")
		}
		parts = append(parts, rest[:end])
		rest = rest[end:]
	}
	out := make([][]byte, len(parts))
	for i, data := range parts {
		f := Fragment{Header: e, Part: Part{Index: uint32(i), Count: uint32(len(parts)), Data: data}}
		if out[i], err = Encode(f); err != nil {
			return nil, err
		}
	}
	return out, nil
}

// RateLimit is Rust's wire::RateLimit for one direction: MaxMessagesPerSecond messages and
// MaxBytesPerSecond bytes, refilled per millisecond, with at most one second of burst.
type RateLimit struct {
	lastMs   uint64
	messages uint64
	bytes    uint64
}

// NewRateLimit gives one second of initial burst credit.
func NewRateLimit(nowMs uint64) RateLimit {
	return RateLimit{
		lastMs:   nowMs,
		messages: MaxMessagesPerSecond * 1000,
		bytes:    MaxBytesPerSecond * 1000,
	}
}

// Charge spends credit for one message of size bytes, before it is parsed. A clock that goes
// back mints no credit, and a message over MaxEnvelopeBytes is refused whatever the credit.
func (r *RateLimit) Charge(size int, nowMs uint64) error {
	return r.ChargeAll([]int{size}, nowMs)
}

// ChargeAll spends credit for messages of sizes, all or none: the fragments of one message are
// sent together or not at all.
func (r *RateLimit) ChargeAll(sizes []int, nowMs uint64) error {
	elapsed := min(nowMs-min(nowMs, r.lastMs), 1000)
	r.lastMs = max(r.lastMs, nowMs)
	r.messages = min(r.messages+elapsed*MaxMessagesPerSecond, MaxMessagesPerSecond*1000)
	r.bytes = min(r.bytes+elapsed*MaxBytesPerSecond, MaxBytesPerSecond*1000)
	var cost uint64
	for _, size := range sizes {
		if size > MaxEnvelopeBytes {
			return errors.New("envelope too large")
		}
		cost += uint64(size) * 1000
	}
	if r.messages < 1000*uint64(len(sizes)) || r.bytes < cost {
		return errors.New("channel rate exceeded")
	}
	r.messages -= 1000 * uint64(len(sizes))
	r.bytes -= cost
	return nil
}

// Recipient is one bundle that can receive envelopes: the permissions it was granted and the
// channels its manifest declares.
type Recipient struct {
	Permissions Permissions
	Channels    []Channel
}

// Grant is what a completed handshake established for one connection: the signed offer, the
// route and wire that Accept bound and the bundles that Ready activated, by package id.
type Grant struct {
	Offer      *Offer
	Session    string
	Connection string
	Subclient  uint8
	Wire       Wire
	Recipients map[string]Recipient
}

// partial is a wire v2 message whose fragments are still arriving.
type partial struct {
	header      Envelope
	count, next uint32
	data        []byte
	// bytes counts the carrier bytes of its fragments so far.
	bytes int
}

// Ingress is one reliable direction of a session, validated as Rust's wire::Ingress validates
// what the client receives. The client's ingress receives ToClient; the server's receives
// ToServer under the same rules.
type Ingress struct {
	direction Direction
	rate      RateLimit
	next      uint64
	partial   *partial
	failed    bool
	// Skipped counts envelopes of an undeclared channel schema.
	Skipped uint64
	// Stale counts envelopes of another world epoch than the receiver's.
	Stale uint64
}

// NewIngress starts a fresh sequence space, from 1, after a signed handshake.
func NewIngress(direction Direction, nowMs uint64) *Ingress {
	return &Ingress{direction: direction, rate: NewRateLimit(nowMs), next: 1}
}

// Receive charges the rate limit, then takes one carrier message: a whole envelope, or on wire
// v2 a fragment. It validates the route, the next sequence number, the bundle and its generation,
// the channel namespace, messaging permission and the record, as Rust's Ingress::receive does,
// and returns the envelope once it is whole. It returns nil with no error for a fragment of a
// message still arriving and for an envelope it skips: an undeclared (channel, schema), counted
// in Skipped, or a world epoch other than worldEpoch, counted in Stale, which Rust drops when it
// dispatches. A skipped envelope still consumes its sequence number. Any error quarantines the
// ingress for good.
func (in *Ingress) Receive(data []byte, nowMs, worldEpoch uint64, grant *Grant) (*Envelope, error) {
	if err := in.charge(len(data), nowMs); err != nil {
		return nil, err
	}
	return in.take(data, worldEpoch, grant)
}

// charge is the rate limit half of Receive.
func (in *Ingress) charge(size int, nowMs uint64) error {
	if in.failed {
		return errors.New("channel quarantined")
	}
	if err := in.rate.Charge(size, nowMs); err != nil {
		in.fail()
		return err
	}
	return nil
}

// take is Receive after the rate limit.
func (in *Ingress) take(data []byte, worldEpoch uint64, grant *Grant) (*Envelope, error) {
	envelope, err := in.receive(data, worldEpoch, grant)
	if err != nil {
		in.fail()
		return nil, err
	}
	return envelope, nil
}

// fail quarantines the ingress and drops what it holds.
func (in *Ingress) fail() {
	in.failed = true
	in.partial = nil
}

func (in *Ingress) receive(data []byte, worldEpoch uint64, grant *Grant) (*Envelope, error) {
	if in.failed {
		return nil, errors.New("channel quarantined")
	}
	if !grant.Offer.Scope.Permissions.Has(PermissionMessaging) {
		return nil, errors.New("messaging permission denied")
	}
	limits := grant.Wire.Limits
	var envelope Envelope
	maxBytes := limits.MaxFragmentBytes
	switch {
	case grant.Wire.Version == WireVersion:
		if err := Decode(data, &envelope); err != nil {
			return nil, err
		}
	case Decode(data, &envelope) == nil:
		if in.partial != nil {
			return nil, errors.New("envelope inside a fragmented message")
		}
	default:
		var f Fragment
		if err := Decode(data, &f); err != nil {
			return nil, err
		}
		whole, err := in.reassemble(f, len(data), grant)
		if whole == nil || err != nil {
			return nil, err
		}
		envelope, maxBytes = *whole, limits.MaxMessageBytes
	}
	if err := in.route(envelope, grant); err != nil {
		return nil, err
	}
	if in.next == ^uint64(0) {
		return nil, errors.New("sequence exhausted")
	}
	in.next++
	if envelope.Generation != InitialBundleGeneration ||
		!slices.ContainsFunc(grant.Offer.Packages, func(p PackageOffer) bool { return p.ID == envelope.Bundle }) {
		return nil, errors.New("wrong bundle generation")
	}
	if !strings.HasPrefix(envelope.Channel, envelope.Bundle+".") {
		return nil, errors.New("foreign channel namespace")
	}
	recipient, ok := grant.Recipients[envelope.Bundle]
	if !ok {
		return nil, errors.New("unknown recipient")
	}
	if !recipient.Permissions.Has(PermissionMessaging) {
		return nil, errors.New("recipient messaging permission denied")
	}
	if len(recipient.Channels) > MaxChannels {
		return nil, errors.New("channel limit exceeded")
	}
	i := slices.IndexFunc(recipient.Channels, func(c Channel) bool {
		return c.ID == envelope.Channel && c.Schema == envelope.Schema
	})
	if i < 0 {
		in.Skipped++
		return nil, nil
	}
	if err := recipient.Channels[i].Validate(envelope.Payload, in.direction, int(maxBytes)); err != nil {
		return nil, err
	}
	if envelope.WorldEpoch != worldEpoch {
		in.Stale++
		return nil, nil
	}
	return &envelope, nil
}

// route checks the session route and that e is the next in sequence.
func (in *Ingress) route(e Envelope, grant *Grant) error {
	if e.Version != grant.Wire.Version || e.Session != grant.Session ||
		e.Connection != grant.Connection || e.Subclient != grant.Subclient {
		return errors.New("wrong session route")
	}
	if e.Sequence != in.next {
		return errors.New("replay or reliable sequence gap")
	}
	return nil
}

// reassemble buffers one fragment, in order and one message at a time, within the per-message
// cap and the per-connection budget, and returns the message once its last fragment is in.
func (in *Ingress) reassemble(f Fragment, size int, grant *Grant) (*Envelope, error) {
	limits := grant.Wire.Limits
	if f.Part.Data == "" || len(f.Part.Data) > int(limits.MaxFragmentBytes) {
		return nil, errors.New("fragment data size")
	}
	p := in.partial
	if p == nil {
		if f.Part.Count < 2 {
			return nil, errors.New("a message of one fragment")
		}
		if err := in.route(f.Header, grant); err != nil {
			return nil, err
		}
		p = &partial{header: f.Header, count: f.Part.Count}
		in.partial = p
	}
	if !sameHeader(f.Header, p.header) || f.Part.Count != p.count || f.Part.Index != p.next {
		return nil, errors.New("fragment out of order")
	}
	p.data = append(p.data, f.Part.Data...)
	p.bytes += size
	p.next++
	if len(p.data) > int(limits.MaxMessageBytes) {
		return nil, errors.New("message too large")
	}
	if p.bytes > int(limits.MaxReassemblyBytes) {
		return nil, errors.New("reassembly budget exceeded")
	}
	if p.next < p.count {
		return nil, nil
	}
	in.partial = nil
	whole := p.header
	if err := Decode(p.data, (*Record)(&whole.Payload)); err != nil {
		return nil, err
	}
	return &whole, nil
}

// open reports whether a fragmented message is still arriving.
func (in *Ingress) open() bool {
	return in.partial != nil
}
