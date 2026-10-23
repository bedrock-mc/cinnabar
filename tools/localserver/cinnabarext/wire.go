package cinnabarext

import (
	"errors"
	"fmt"
	"slices"
	"strings"
	"unicode/utf8"
)

// Record is a Rust Vec<wire::Scalar>: an envelope's payload, which the client part's guest sends
// and receives as record-json.
type Record []Scalar

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
