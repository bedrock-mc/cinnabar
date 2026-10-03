package extension

import (
	"errors"
	"fmt"
	"slices"
	"strconv"
	"strings"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/experience"
)

// appendScalar appends s as Rust serializes a wire::Scalar: {"type":…,"value":…}. A scalar with
// no field or more than one set has no Rust value.
func appendScalar(b []byte, s experience.Scalar) ([]byte, error) {
	set := 0
	for _, field := range []bool{s.Bool != nil, s.Integer != nil, s.Text != nil, s.Choice != nil} {
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
	switch kind {
	case "bool":
		value = func(r *reader) error { v, err := r.boolean(); s.Bool = &v; return err }
	case "integer":
		value = func(r *reader) error { v, err := r.int64(); s.Integer = &v; return err }
	case "text":
		value = func(r *reader) error { v, err := r.str(); s.Text = &v; return err }
	case "choice":
		value = func(r *reader) error { v, err := readUint[uint16](r); s.Choice = &v; return err }
	default:
		return experience.Scalar{}, r.errorf("unknown scalar type %q", kind)
	}
	err = within(raw, func(sub *reader) error {
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
// IntegerField, TextField or ChoiceField.
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

func (BoolField) field()    {}
func (IntegerField) field() {}
func (TextField) field()    {}
func (ChoiceField) field()  {}

// fieldMembers are the serde names of each field type's members, tag first.
var fieldMembers = map[string][]string{
	"bool":    {"type"},
	"integer": {"type", "min", "max"},
	"text":    {"type", "max_bytes"},
	"choice":  {"type", "variants"},
}

// fieldList is a Rust Vec<wire::Field>.
type fieldList []Field

func (l fieldList) appendJSON(b []byte) ([]byte, error) {
	b = append(b, '[')
	for i, field := range l {
		if i > 0 {
			b = append(b, ',')
		}
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
		default:
			return nil, fmt.Errorf("field %d has no type", i)
		}
	}
	return append(b, ']'), nil
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
	err = within(raw, func(sub *reader) error {
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
	}
	return BoolField{}, nil
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
			err = r.array(func() error {
				field, err := readField(r)
				c.Fields = append(c.Fields, field)
				return err
			})
		}
		return err
	})
}

// Validate checks payload against the channel as Rust's Channel::validate does before guest
// dispatch or sending: an identifier id, the declared direction, at most MaxChannelFields fields,
// one value of the declared type and range per field and at most MaxPayloadBytes of encoded
// record.
func (c Channel) Validate(payload []experience.Scalar, direction Direction) error {
	if !Identifier(c.ID) || c.Direction != direction || len(c.Fields) > MaxChannelFields {
		return errors.New("channel denied")
	}
	if len(payload) != len(c.Fields) {
		return errors.New("record field count mismatch")
	}
	for i, field := range c.Fields {
		value := payload[i]
		valid := false
		switch f := field.(type) {
		case BoolField:
			valid = value.Bool != nil
		case IntegerField:
			valid = value.Integer != nil && f.Min <= *value.Integer && *value.Integer <= f.Max
		case TextField:
			valid = value.Text != nil && len(*value.Text) <= int(f.MaxBytes)
		case ChoiceField:
			valid = value.Choice != nil && *value.Choice < f.Variants
		}
		if !valid {
			return fmt.Errorf("record field %d rejected", i)
		}
	}
	encoded, err := Encode(Record(payload))
	if err != nil {
		return err
	}
	if len(encoded) > MaxPayloadBytes {
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

func (e Envelope) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: envelopeFields}
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
	o.value(Record(e.Payload))
	return o.end()
}

func (e *Envelope) decodeJSON(r *reader) error {
	*e = Envelope{}
	return r.fields(envelopeFields, func(name string) (err error) {
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
		case "payload":
			err = (*Record)(&e.Payload).decodeJSON(r)
		}
		return err
	})
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
	elapsed := min(nowMs-min(nowMs, r.lastMs), 1000)
	r.lastMs = max(r.lastMs, nowMs)
	r.messages = min(r.messages+elapsed*MaxMessagesPerSecond, MaxMessagesPerSecond*1000)
	r.bytes = min(r.bytes+elapsed*MaxBytesPerSecond, MaxBytesPerSecond*1000)
	if size > MaxEnvelopeBytes {
		return errors.New("envelope too large")
	}
	cost := uint64(size) * 1000
	if r.messages < 1000 || r.bytes < cost {
		return errors.New("channel rate exceeded")
	}
	r.messages -= 1000
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
// route that Accept bound and the bundles that Ready activated, by package id.
type Grant struct {
	Offer      *Offer
	Session    string
	Connection string
	Subclient  uint8
	Recipients map[string]Recipient
}

// Ingress is one reliable direction of a session, validated as Rust's wire::Ingress validates
// what the client receives. The client's ingress receives ToClient; the server's receives
// ToServer under the same rules.
type Ingress struct {
	direction Direction
	rate      RateLimit
	next      uint64
	failed    bool
	// Skipped counts envelopes of an undeclared channel schema or of another world epoch.
	Skipped uint64
}

// NewIngress starts a fresh sequence space, from 1, after a signed handshake.
func NewIngress(direction Direction, nowMs uint64) *Ingress {
	return &Ingress{direction: direction, rate: NewRateLimit(nowMs), next: 1}
}

// Receive charges the rate limit, then validates one envelope: the route, the next sequence
// number, the bundle and its generation, the channel namespace, messaging permission and the
// record. It returns nil with no error for an envelope it skips: an undeclared (channel, schema),
// or a world epoch other than worldEpoch, which Rust drops when it dispatches. A skipped envelope
// still consumes its sequence number. Any error quarantines the ingress for good.
func (in *Ingress) Receive(data []byte, nowMs, worldEpoch uint64, grant *Grant) (*Envelope, error) {
	envelope, err := in.receive(data, nowMs, worldEpoch, grant)
	if err != nil {
		in.failed = true
		return nil, err
	}
	return envelope, nil
}

func (in *Ingress) receive(data []byte, nowMs, worldEpoch uint64, grant *Grant) (*Envelope, error) {
	if in.failed {
		return nil, errors.New("channel quarantined")
	}
	if err := in.rate.Charge(len(data), nowMs); err != nil {
		return nil, err
	}
	if !grant.Offer.Scope.Permissions.Has(PermissionMessaging) {
		return nil, errors.New("messaging permission denied")
	}
	var envelope Envelope
	if err := Decode(data, &envelope); err != nil {
		return nil, err
	}
	if envelope.Version != WireVersion || envelope.Session != grant.Session ||
		envelope.Connection != grant.Connection || envelope.Subclient != grant.Subclient {
		return nil, errors.New("wrong session route")
	}
	if envelope.Sequence != in.next {
		return nil, errors.New("replay or reliable sequence gap")
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
	if err := recipient.Channels[i].Validate(envelope.Payload, in.direction); err != nil {
		return nil, err
	}
	if envelope.WorldEpoch != worldEpoch {
		in.Skipped++
		return nil, nil
	}
	return &envelope, nil
}
