// Generated from crates/server-experience/schema/extension.json; do not edit.
package cinnabarext

import (
	"errors"
	"fmt"
	"slices"
	"strconv"
)

// Channel is the schema-defined Channel document.
type Channel struct {
	ID        string
	Schema    uint16
	Direction Direction
	Fields    []Field
}

var channelFields = []string{"id", "schema", "direction", "fields"}

// appendJSON writes Channel in canonical field order.
func (v Channel) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: channelFields}
	o.str(v.ID)
	o.uint(uint64(v.Schema))
	o.value(v.Direction)
	o.value(fieldList(v.Fields))
	return o.end()
}

// decodeJSON reads a strict Channel object.
func (v *Channel) decodeJSON(r *reader) error {
	*v = Channel{}
	return r.fields(channelFields, func(name string) (err error) {
		switch name {
		case "id":
			v.ID, err = r.str()
		case "schema":
			v.Schema, err = readUint[uint16](r)
		case "direction":
			err = v.Direction.decodeJSON(r)
		case "fields":
			v.Fields, err = readFields(r)
		}
		return err
	})
}

// Envelope is the schema-defined Envelope document.
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
	Payload    []Scalar
}

var envelopeFields = []string{"version", "session", "connection", "subclient", "bundle", "generation", "channel", "schema", "sequence", "world_epoch", "payload"}

// appendJSON writes Envelope in canonical field order.
func (v Envelope) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: envelopeFields}
	o.uint(uint64(v.Version))
	o.str(v.Session)
	o.str(v.Connection)
	o.uint(uint64(v.Subclient))
	o.str(v.Bundle)
	o.uint(uint64(v.Generation))
	o.str(v.Channel)
	o.uint(uint64(v.Schema))
	o.uint(uint64(v.Sequence))
	o.uint(uint64(v.WorldEpoch))
	o.value(Record(v.Payload))
	return o.end()
}

// decodeJSON reads a strict Envelope object.
func (v *Envelope) decodeJSON(r *reader) error {
	*v = Envelope{}
	return r.fields(envelopeFields, func(name string) (err error) {
		switch name {
		case "version":
			v.Version, err = readUint[uint16](r)
		case "session":
			v.Session, err = r.str()
		case "connection":
			v.Connection, err = r.str()
		case "subclient":
			v.Subclient, err = readUint[uint8](r)
		case "bundle":
			v.Bundle, err = r.str()
		case "generation":
			v.Generation, err = readUint[uint64](r)
		case "channel":
			v.Channel, err = r.str()
		case "schema":
			v.Schema, err = readUint[uint16](r)
		case "sequence":
			v.Sequence, err = readUint[uint64](r)
		case "world_epoch":
			v.WorldEpoch, err = readUint[uint64](r)
		case "payload":
			err = (*Record)(&v.Payload).decodeJSON(r)
		}
		return err
	})
}

// Part is the schema-defined Part document.
type Part struct {
	Index uint32
	Count uint32
	Data  string
}

var partFields = []string{"index", "count", "data"}

// appendJSON writes Part in canonical field order.
func (v Part) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: partFields}
	o.uint(uint64(v.Index))
	o.uint(uint64(v.Count))
	o.str(v.Data)
	return o.end()
}

// decodeJSON reads a strict Part object.
func (v *Part) decodeJSON(r *reader) error {
	*v = Part{}
	return r.fields(partFields, func(name string) (err error) {
		switch name {
		case "index":
			v.Index, err = readUint[uint32](r)
		case "count":
			v.Count, err = readUint[uint32](r)
		case "data":
			v.Data, err = r.str()
		}
		return err
	})
}

// Fragment is the schema-defined Fragment document.
type Fragment struct {
	Header Envelope
	Part   Part
}

var fragmentFields = []string{"version", "session", "connection", "subclient", "bundle", "generation", "channel", "schema", "sequence", "world_epoch", "fragment"}

// appendJSON writes Fragment in canonical field order.
func (v Fragment) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: fragmentFields}
	o.uint(uint64(v.Header.Version))
	o.str(v.Header.Session)
	o.str(v.Header.Connection)
	o.uint(uint64(v.Header.Subclient))
	o.str(v.Header.Bundle)
	o.uint(uint64(v.Header.Generation))
	o.str(v.Header.Channel)
	o.uint(uint64(v.Header.Schema))
	o.uint(uint64(v.Header.Sequence))
	o.uint(uint64(v.Header.WorldEpoch))
	o.value(v.Part)
	return o.end()
}

// decodeJSON reads a strict Fragment object.
func (v *Fragment) decodeJSON(r *reader) error {
	*v = Fragment{}
	return r.fields(fragmentFields, func(name string) (err error) {
		switch name {
		case "version":
			v.Header.Version, err = readUint[uint16](r)
		case "session":
			v.Header.Session, err = r.str()
		case "connection":
			v.Header.Connection, err = r.str()
		case "subclient":
			v.Header.Subclient, err = readUint[uint8](r)
		case "bundle":
			v.Header.Bundle, err = r.str()
		case "generation":
			v.Header.Generation, err = readUint[uint64](r)
		case "channel":
			v.Header.Channel, err = r.str()
		case "schema":
			v.Header.Schema, err = readUint[uint16](r)
		case "sequence":
			v.Header.Sequence, err = readUint[uint64](r)
		case "world_epoch":
			v.Header.WorldEpoch, err = readUint[uint64](r)
		case "fragment":
			err = v.Part.decodeJSON(r)
		}
		return err
	})
}

type Direction uint8

const (
	ToClient Direction = iota
	ToServer
)

var directionNames = [...]string{"to_client", "to_server"}

// String returns the schema name, or a diagnostic for an invalid enum.
func (v Direction) String() string {
	if int(v) < len(directionNames) {
		return directionNames[v]
	}
	return fmt.Sprintf("Direction(%d)", uint8(v))
}

// appendJSON encodes one declared enum name.
func (v Direction) appendJSON(b []byte) ([]byte, error) {
	if int(v) >= len(directionNames) {
		return nil, fmt.Errorf("invalid Direction %d", v)
	}
	return appendString(b, directionNames[v])
}

// decodeJSON rejects names outside the schema.
func (v *Direction) decodeJSON(r *reader) error {
	name, err := r.str()
	if err != nil {
		return err
	}
	i := slices.Index(directionNames[:], name)
	if i < 0 {
		return r.errorf("unknown Direction %q", name)
	}
	*v = Direction(i)
	return nil
}

var scalarFields = []string{"type", "value"}

// appendScalar requires exactly one value and writes its schema tag.
func appendScalar(b []byte, s Scalar) ([]byte, error) {
	count := 0
	if s.Bool != nil {
		count++
	}
	if s.Integer != nil {
		count++
	}
	if s.Text != nil {
		count++
	}
	if s.Choice != nil {
		count++
	}
	if s.List != nil {
		count++
	}
	if s.Record != nil {
		count++
	}
	if count != 1 {
		return nil, fmt.Errorf("a scalar holds %d values, not 1", count)
	}
	o := object{b: b, names: scalarFields}
	switch {
	case s.Bool != nil:
		o.str("bool")
		o.key()
		o.b = strconv.AppendBool(o.b, *s.Bool)
	case s.Integer != nil:
		o.str("integer")
		o.int(*s.Integer)
	case s.Text != nil:
		o.str("text")
		o.str(*s.Text)
	case s.Choice != nil:
		o.str("choice")
		o.uint(uint64(*s.Choice))
	case s.List != nil:
		o.str("list")
		o.value(Record(*s.List))
	case s.Record != nil:
		o.str("record")
		o.value(Record(*s.Record))
	}
	return o.end()
}

// readScalar rejects undeclared tags and preserves the recursion budget.
func readScalar(r *reader) (Scalar, error) {
	kind, raw, err := r.tagged("type")
	if err != nil {
		return Scalar{}, err
	}
	var s Scalar
	err = within(raw, func(sub *reader) error {
		sub.depth = r.depth
		return sub.fields(scalarFields, func(name string) (err error) {
			if name == "type" {
				_, err = sub.str()
				return err
			}
			switch kind {
			case "bool":
				var value bool
				value, err = sub.boolean()
				s.Bool = &value
			case "integer":
				var value int64
				value, err = sub.int64()
				s.Integer = &value
			case "text":
				var value string
				value, err = sub.str()
				s.Text = &value
			case "choice":
				var value uint16
				value, err = readUint[uint16](sub)
				s.Choice = &value
			case "list":
				var value []Scalar
				err = (*Record)(&value).decodeJSON(sub)
				if value == nil {
					value = []Scalar{}
				}
				s.List = &value
			case "record":
				var value []Scalar
				err = (*Record)(&value).decodeJSON(sub)
				if value == nil {
					value = []Scalar{}
				}
				s.Record = &value
			default:
				return sub.errorf("unknown scalar type %q", kind)
			}
			return err
		})
	})
	if err != nil {
		return Scalar{}, err
	}
	return s, nil
}

type Field interface{ field() }

var fieldMembers = map[string][]string{
	"bool":    {"type"},
	"integer": {"type", "min", "max"},
	"text":    {"type", "max_bytes"},
	"choice":  {"type", "variants"},
	"list":    {"type", "item", "max_items"},
	"record":  {"type", "fields"},
}

type BoolField struct {
}

// field identifies a declared field variant.
func (BoolField) field() {}

type IntegerField struct {
	Min int64
	Max int64
}

// field identifies a declared field variant.
func (IntegerField) field() {}

type TextField struct {
	MaxBytes uint16
}

// field identifies a declared field variant.
func (TextField) field() {}

type ChoiceField struct {
	Variants uint16
}

// field identifies a declared field variant.
func (ChoiceField) field() {}

type ListField struct {
	Item     Field
	MaxItems uint16
}

// field identifies a declared field variant.
func (ListField) field() {}

type RecordField struct {
	Fields []Field
}

// field identifies a declared field variant.
func (RecordField) field() {}

// appendField writes an internally tagged field declaration.
func appendField(b []byte, field Field) ([]byte, error) {
	switch f := field.(type) {
	case BoolField:
		o := object{b: b, names: fieldMembers["bool"]}
		o.str("bool")
		_ = f
		return o.end()
	case IntegerField:
		o := object{b: b, names: fieldMembers["integer"]}
		o.str("integer")
		o.int(f.Min)
		o.int(f.Max)
		return o.end()
	case TextField:
		o := object{b: b, names: fieldMembers["text"]}
		o.str("text")
		o.uint(uint64(f.MaxBytes))
		return o.end()
	case ChoiceField:
		o := object{b: b, names: fieldMembers["choice"]}
		o.str("choice")
		o.uint(uint64(f.Variants))
		return o.end()
	case ListField:
		o := object{b: b, names: fieldMembers["list"]}
		o.str("list")
		o.key()
		o.add(appendField(o.b, f.Item))
		o.uint(uint64(f.MaxItems))
		return o.end()
	case RecordField:
		o := object{b: b, names: fieldMembers["record"]}
		o.str("record")
		o.value(fieldList(f.Fields))
		return o.end()
	}
	return nil, errors.New("a field with no type")
}

// readField reads only the members of the declared variant.
func readField(r *reader) (Field, error) {
	kind, raw, err := r.tagged("type")
	if err != nil {
		return nil, err
	}
	switch kind {
	case "bool":
		var v BoolField
		err = within(raw, func(sub *reader) error {
			sub.depth = r.depth
			return sub.fields(fieldMembers[kind], func(name string) (err error) {
				switch name {
				case "type":
					_, err = sub.str()
				}
				return err
			})
		})
		if err != nil {
			return nil, err
		}
		return v, nil
	case "integer":
		var v IntegerField
		err = within(raw, func(sub *reader) error {
			sub.depth = r.depth
			return sub.fields(fieldMembers[kind], func(name string) (err error) {
				switch name {
				case "type":
					_, err = sub.str()
				case "min":
					v.Min, err = sub.int64()
				case "max":
					v.Max, err = sub.int64()
				}
				return err
			})
		})
		if err != nil {
			return nil, err
		}
		return v, nil
	case "text":
		var v TextField
		err = within(raw, func(sub *reader) error {
			sub.depth = r.depth
			return sub.fields(fieldMembers[kind], func(name string) (err error) {
				switch name {
				case "type":
					_, err = sub.str()
				case "max_bytes":
					v.MaxBytes, err = readUint[uint16](sub)
				}
				return err
			})
		})
		if err != nil {
			return nil, err
		}
		return v, nil
	case "choice":
		var v ChoiceField
		err = within(raw, func(sub *reader) error {
			sub.depth = r.depth
			return sub.fields(fieldMembers[kind], func(name string) (err error) {
				switch name {
				case "type":
					_, err = sub.str()
				case "variants":
					v.Variants, err = readUint[uint16](sub)
				}
				return err
			})
		})
		if err != nil {
			return nil, err
		}
		return v, nil
	case "list":
		var v ListField
		err = within(raw, func(sub *reader) error {
			sub.depth = r.depth
			return sub.fields(fieldMembers[kind], func(name string) (err error) {
				switch name {
				case "type":
					_, err = sub.str()
				case "item":
					v.Item, err = readField(sub)
				case "max_items":
					v.MaxItems, err = readUint[uint16](sub)
				}
				return err
			})
		})
		if err != nil {
			return nil, err
		}
		return v, nil
	case "record":
		var v RecordField
		err = within(raw, func(sub *reader) error {
			sub.depth = r.depth
			return sub.fields(fieldMembers[kind], func(name string) (err error) {
				switch name {
				case "type":
					_, err = sub.str()
				case "fields":
					v.Fields, err = readFields(sub)
				}
				return err
			})
		})
		if err != nil {
			return nil, err
		}
		return v, nil
	}
	return nil, r.errorf("unknown field type %q", kind)
}

// declared checks container nesting and record field counts.
func declared(f Field, depth int) bool {
	switch f := f.(type) {
	case ListField:
		return depth > 0 && declared(f.Item, depth-1)
	case RecordField:
		return depth > 0 && len(f.Fields) <= MaxChannelFields && !slices.ContainsFunc(f.Fields, func(field Field) bool { return !declared(field, depth-1) })
	case BoolField, IntegerField, TextField, ChoiceField:
		return true
	}
	return false
}

// admits checks the value's type and every schema-defined bound recursively.
func admits(f Field, value Scalar) bool {
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
		return value.List != nil && len(*value.List) <= int(f.MaxItems) && !slices.ContainsFunc(*value.List, func(value Scalar) bool { return !admits(f.Item, value) })
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

// Declared checks identifiers, field counts and nesting against the shared ceilings.
func (c Channel) Declared() bool {
	return Identifier(c.ID) && len(c.Fields) <= MaxChannelFields && !slices.ContainsFunc(c.Fields, func(f Field) bool { return !declared(f, MaxFieldDepth) })
}

// Validate checks direction, positional fields and the encoded byte ceiling.
func (c Channel) Validate(payload []Scalar, direction Direction, maxBytes int) error {
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
