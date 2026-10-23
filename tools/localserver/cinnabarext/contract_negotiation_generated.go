// Generated from crates/server-experience/schema/extension.json; do not edit.
package cinnabarext

// Marker is the schema-defined Marker document.
type Marker struct {
	ServerKey string
	Offer     SignedDocument
}

var markerFields = []string{"server_key", "offer"}

// appendJSON writes Marker in canonical field order.
func (v Marker) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: markerFields}
	o.str(v.ServerKey)
	o.value(v.Offer)
	return o.end()
}

// decodeJSON reads a strict Marker object.
func (v *Marker) decodeJSON(r *reader) error {
	*v = Marker{}
	return r.fields(markerFields, func(name string) (err error) {
		switch name {
		case "server_key":
			v.ServerKey, err = r.str()
		case "offer":
			err = v.Offer.decodeJSON(r)
		}
		return err
	})
}

// Limits is the schema-defined Limits document.
type Limits struct {
	MaxFragmentBytes   uint32
	MaxMessageBytes    uint32
	MaxReassemblyBytes uint32
}

var limitsFields = []string{"max_fragment_bytes", "max_message_bytes", "max_reassembly_bytes"}

// appendJSON writes Limits in canonical field order.
func (v Limits) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: limitsFields}
	o.uint(uint64(v.MaxFragmentBytes))
	o.uint(uint64(v.MaxMessageBytes))
	o.uint(uint64(v.MaxReassemblyBytes))
	return o.end()
}

// decodeJSON reads a strict Limits object.
func (v *Limits) decodeJSON(r *reader) error {
	*v = Limits{}
	return r.fields(limitsFields, func(name string) (err error) {
		switch name {
		case "max_fragment_bytes":
			v.MaxFragmentBytes, err = readUint[uint32](r)
		case "max_message_bytes":
			v.MaxMessageBytes, err = readUint[uint32](r)
		case "max_reassembly_bytes":
			v.MaxReassemblyBytes, err = readUint[uint32](r)
		}
		return err
	})
}

// WireOffer is the schema-defined WireOffer document.
type WireOffer struct {
	Versions []uint16
	Limits   Limits
}

var wireOfferFields = []string{"versions", "limits"}

// appendJSON writes WireOffer in canonical field order.
func (v WireOffer) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: wireOfferFields}
	o.key()
	o.add(appendVersions(o.b, v.Versions))
	o.value(v.Limits)
	return o.end()
}

// decodeJSON reads a strict WireOffer object.
func (v *WireOffer) decodeJSON(r *reader) error {
	*v = WireOffer{}
	return r.fields(wireOfferFields, func(name string) (err error) {
		switch name {
		case "versions":
			v.Versions, err = readVersions(r)
		case "limits":
			err = v.Limits.decodeJSON(r)
		}
		return err
	})
}

// Wire is the schema-defined Wire document.
type Wire struct {
	Version uint16
	Limits  Limits
}

var wireFields = []string{"version", "limits"}

// appendJSON writes Wire in canonical field order.
func (v Wire) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: wireFields}
	o.uint(uint64(v.Version))
	o.value(v.Limits)
	return o.end()
}

// decodeJSON reads a strict Wire object.
func (v *Wire) decodeJSON(r *reader) error {
	*v = Wire{}
	return r.fields(wireFields, func(name string) (err error) {
		switch name {
		case "version":
			v.Version, err = readUint[uint16](r)
		case "limits":
			err = v.Limits.decodeJSON(r)
		}
		return err
	})
}

// Hello is the schema-defined Hello document.
type Hello struct {
	Version         uint16
	API             uint16
	Capabilities    Permissions
	OfferDigest     string
	ClientChallenge string
	Connection      string
	Subclient       uint8
	Wire            *WireOffer
}

var helloFields = []string{"version", "api", "capabilities", "offer_digest", "client_challenge", "connection", "subclient", "wire"}

// appendJSON writes Hello in canonical field order.
func (v Hello) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: helloFields}
	o.uint(uint64(v.Version))
	o.uint(uint64(v.API))
	o.value(v.Capabilities)
	o.str(v.OfferDigest)
	o.str(v.ClientChallenge)
	o.str(v.Connection)
	o.uint(uint64(v.Subclient))
	if v.Wire != nil {
		o.value(*v.Wire)
	} else {
		o.skip()
	}
	return o.end()
}

// decodeJSON reads a strict Hello object.
func (v *Hello) decodeJSON(r *reader) error {
	*v = Hello{}
	return r.fields(helloFields, func(name string) (err error) {
		switch name {
		case "version":
			v.Version, err = readUint[uint16](r)
		case "api":
			v.API, err = readUint[uint16](r)
		case "capabilities":
			err = v.Capabilities.decodeJSON(r)
		case "offer_digest":
			v.OfferDigest, err = r.str()
		case "client_challenge":
			v.ClientChallenge, err = r.str()
		case "connection":
			v.Connection, err = r.str()
		case "subclient":
			v.Subclient, err = readUint[uint8](r)
		case "wire":
			if !r.null() {
				var item WireOffer
				err = item.decodeJSON(r)
				v.Wire = &item
			}
		}
		return err
	}, "wire")
}

// Accept is the schema-defined Accept document.
type Accept struct {
	Hello           Hello
	ServerChallenge string
	Session         string
	Audience        string
	OfferDigest     string
	Revision        uint64
	ExpiresUnix     uint64
	Wire            *Wire
}

var acceptFields = []string{"hello", "server_challenge", "session", "audience", "offer_digest", "revision", "expires_unix", "wire"}

// appendJSON writes Accept in canonical field order.
func (v Accept) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: acceptFields}
	o.value(v.Hello)
	o.str(v.ServerChallenge)
	o.str(v.Session)
	o.str(v.Audience)
	o.str(v.OfferDigest)
	o.uint(uint64(v.Revision))
	o.uint(uint64(v.ExpiresUnix))
	if v.Wire != nil {
		o.value(*v.Wire)
	} else {
		o.skip()
	}
	return o.end()
}

// decodeJSON reads a strict Accept object.
func (v *Accept) decodeJSON(r *reader) error {
	*v = Accept{}
	return r.fields(acceptFields, func(name string) (err error) {
		switch name {
		case "hello":
			err = v.Hello.decodeJSON(r)
		case "server_challenge":
			v.ServerChallenge, err = r.str()
		case "session":
			v.Session, err = r.str()
		case "audience":
			v.Audience, err = r.str()
		case "offer_digest":
			v.OfferDigest, err = r.str()
		case "revision":
			v.Revision, err = readUint[uint64](r)
		case "expires_unix":
			v.ExpiresUnix, err = readUint[uint64](r)
		case "wire":
			if !r.null() {
				var item Wire
				err = item.decodeJSON(r)
				v.Wire = &item
			}
		}
		return err
	}, "wire")
}

// ordered checks that the selected ceilings are nonzero and ordered.
func (l Limits) ordered() bool {
	return 0 < l.MaxFragmentBytes && l.MaxFragmentBytes <= l.MaxMessageBytes && l.MaxMessageBytes <= l.MaxReassemblyBytes
}

// lowest lowers each selected ceiling to the corresponding offer ceiling.
func (l Limits) lowest(other Limits) Limits {
	return Limits{min(l.MaxFragmentBytes, other.MaxFragmentBytes), min(l.MaxMessageBytes, other.MaxMessageBytes), min(l.MaxReassemblyBytes, other.MaxReassemblyBytes)}
}
