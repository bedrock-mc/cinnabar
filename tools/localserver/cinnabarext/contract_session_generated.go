// Generated from crates/server-experience/schema/extension.json; do not edit.
package cinnabarext

import (
	"fmt"
)

// Ready is the schema-defined Ready document.
type Ready struct {
	Session     string
	Packages    []string
	Generation  uint64
	Permissions map[string]Permissions
	WorldEpoch  uint64
}

var readyFields = []string{"session", "packages", "generation", "permissions", "world_epoch"}

// appendJSON writes Ready in canonical field order.
func (v Ready) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: readyFields}
	o.str(v.Session)
	o.key()
	o.add(appendStrings(o.b, v.Packages))
	o.uint(uint64(v.Generation))
	o.key()
	o.add(appendPermissionMap(o.b, v.Permissions))
	o.uint(uint64(v.WorldEpoch))
	return o.end()
}

// decodeJSON reads a strict Ready object.
func (v *Ready) decodeJSON(r *reader) error {
	*v = Ready{}
	return r.fields(readyFields, func(name string) (err error) {
		switch name {
		case "session":
			v.Session, err = r.str()
		case "packages":
			v.Packages, err = readStrings(r)
		case "generation":
			v.Generation, err = readUint[uint64](r)
		case "permissions":
			v.Permissions, err = readPermissionMap(r)
		case "world_epoch":
			v.WorldEpoch, err = readUint[uint64](r)
		}
		return err
	})
}

// Epoch is the schema-defined Epoch document.
type Epoch struct {
	Session    string
	WorldEpoch uint64
}

var epochFields = []string{"session", "world_epoch"}

// appendJSON writes Epoch in canonical field order.
func (v Epoch) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: epochFields}
	o.str(v.Session)
	o.uint(uint64(v.WorldEpoch))
	return o.end()
}

// decodeJSON reads a strict Epoch object.
func (v *Epoch) decodeJSON(r *reader) error {
	*v = Epoch{}
	return r.fields(epochFields, func(name string) (err error) {
		switch name {
		case "session":
			v.Session, err = r.str()
		case "world_epoch":
			v.WorldEpoch, err = readUint[uint64](r)
		}
		return err
	})
}

// WorldAck is the schema-defined WorldAck document.
type WorldAck struct {
	Session        string
	WorldEpoch     uint64
	RequestedEpoch uint64
}

var worldAckFields = []string{"session", "world_epoch", "requested_epoch"}

// appendJSON writes WorldAck in canonical field order.
func (v WorldAck) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: worldAckFields}
	o.str(v.Session)
	o.uint(uint64(v.WorldEpoch))
	o.uint(uint64(v.RequestedEpoch))
	return o.end()
}

// decodeJSON reads a strict WorldAck object.
func (v *WorldAck) decodeJSON(r *reader) error {
	*v = WorldAck{}
	return r.fields(worldAckFields, func(name string) (err error) {
		switch name {
		case "session":
			v.Session, err = r.str()
		case "world_epoch":
			v.WorldEpoch, err = readUint[uint64](r)
		case "requested_epoch":
			v.RequestedEpoch, err = readUint[uint64](r)
		}
		return err
	})
}

type Control struct {
	Hello    *Hello
	Accept   *SignedDocument
	Ready    *Ready
	Epoch    *Epoch
	WorldAck *WorldAck
	Disabled bool
}

var controlFields = []string{"kind", "body"}

// appendJSON encodes exactly one control variant.
func (c Control) appendJSON(b []byte) ([]byte, error) {
	var kind string
	var body CanonicalValue
	count := 0
	if c.Hello != nil {
		kind, body = "hello", *c.Hello
		count++
	}
	if c.Accept != nil {
		kind, body = "accept", *c.Accept
		count++
	}
	if c.Ready != nil {
		kind, body = "ready", *c.Ready
		count++
	}
	if c.Epoch != nil {
		kind, body = "epoch", *c.Epoch
		count++
	}
	if c.WorldAck != nil {
		kind, body = "world_ack", *c.WorldAck
		count++
	}
	if c.Disabled {
		kind, body = "disabled", nil
		count++
	}
	if count != 1 {
		return nil, fmt.Errorf("a control message holds %d variants, not 1", count)
	}
	b = append(b, `{"kind":"`...)
	b = append(b, kind...)
	b = append(b, '"')
	if body != nil {
		b = append(b, `,"body":`...)
		var err error
		b, err = body.appendJSON(b)
		if err != nil {
			return nil, err
		}
	}
	return append(b, '}'), nil
}

// decodeJSON reads the tagged body and rejects unknown or duplicate members.
func (c *Control) decodeJSON(r *reader) error {
	*c = Control{}
	kind, raw, err := r.tagged("kind")
	if err != nil {
		return err
	}
	var body Target
	switch kind {
	case "hello":
		c.Hello = new(Hello)
		body = c.Hello
	case "accept":
		c.Accept = new(SignedDocument)
		body = c.Accept
	case "ready":
		c.Ready = new(Ready)
		body = c.Ready
	case "epoch":
		c.Epoch = new(Epoch)
		body = c.Epoch
	case "world_ack":
		c.WorldAck = new(WorldAck)
		body = c.WorldAck
	case "disabled":
		c.Disabled = true
	default:
		return r.errorf("unknown control kind %q", kind)
	}
	return within(raw, func(sub *reader) error {
		member := func(name string) error {
			if name == "kind" {
				_, err := sub.str()
				return err
			}
			if body != nil {
				return body.decodeJSON(sub)
			}
			if sub.null() {
				return nil
			}
			return sub.errorf("a disabled message has no body")
		}
		if body == nil {
			return sub.fields(controlFields, member, "body")
		}
		return sub.fields(controlFields, member)
	})
}
