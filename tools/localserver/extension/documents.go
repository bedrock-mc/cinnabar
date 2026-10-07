package extension

import (
	"fmt"
	"maps"
	"slices"
	"strconv"
)

// Permission is a Rust manifest::Permission.
type Permission uint8

// The permissions, in Rust's declaration order.
const (
	PermissionUI Permission = iota
	PermissionModalUI
	PermissionInput
	PermissionMessaging
	PermissionScene
	PermissionMedia
)

// permissionNames are the serde names of the permissions, indexed by Permission.
var permissionNames = [...]string{"ui", "modal_ui", "input", "messaging", "scene", "media"}

func (p Permission) String() string {
	if int(p) < len(permissionNames) {
		return permissionNames[p]
	}
	return fmt.Sprintf("Permission(%d)", uint8(p))
}

// Permissions is a Rust BTreeSet<Permission>: one bit per Permission. It encodes in declaration
// order, which is the set's order.
type Permissions uint8

// ImplementedPermissions are the permissions that have client adapters: Rust's
// implemented_permissions(). A developer client's Hello offers exactly these.
const ImplementedPermissions = Permissions(1<<PermissionUI | 1<<PermissionModalUI | 1<<PermissionInput | 1<<PermissionMessaging)

// NewPermissions returns the set of ps.
func NewPermissions(ps ...Permission) Permissions {
	var s Permissions
	for _, p := range ps {
		s |= 1 << p
	}
	return s
}

// Has reports whether p is in s.
func (s Permissions) Has(p Permission) bool {
	return s&(1<<p) != 0
}

// List returns the permissions of s in order.
func (s Permissions) List() []Permission {
	var list []Permission
	for i := range permissionNames {
		if s.Has(Permission(i)) {
			list = append(list, Permission(i))
		}
	}
	return list
}

func (s Permissions) appendJSON(b []byte) ([]byte, error) {
	if s>>len(permissionNames) != 0 {
		return nil, fmt.Errorf("permissions %#x hold a permission Rust does not have", uint8(s))
	}
	b = append(b, '[')
	first := true
	for i, name := range permissionNames {
		if !s.Has(Permission(i)) {
			continue
		}
		if !first {
			b = append(b, ',')
		}
		first = false
		b = append(b, '"')
		b = append(b, name...)
		b = append(b, '"')
	}
	return append(b, ']'), nil
}

func (s *Permissions) decodeJSON(r *reader) error {
	*s = 0
	return r.array(func() error {
		name, err := r.str()
		if err != nil {
			return err
		}
		i := slices.Index(permissionNames[:], name)
		if i < 0 {
			return r.errorf("unknown permission %q", name)
		}
		*s |= 1 << i
		return nil
	})
}

// Scope is a Rust manifest::Scope, the user-visible grant of an offer.
type Scope struct {
	Permissions Permissions
	// Origins is a set of canonical HTTPS origins; it encodes sorted.
	Origins     []string
	MemoryBytes uint64
	GPUBytes    uint64
}

var scopeFields = []string{"permissions", "origins", "memory_bytes", "gpu_bytes"}

func (s Scope) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: scopeFields}
	o.value(s.Permissions)
	o.key()
	o.add(appendSet(o.b, s.Origins))
	o.uint(s.MemoryBytes)
	o.uint(s.GPUBytes)
	return o.end()
}

func (s *Scope) decodeJSON(r *reader) error {
	*s = Scope{}
	return r.fields(scopeFields, func(name string) (err error) {
		switch name {
		case "permissions":
			err = s.Permissions.decodeJSON(r)
		case "origins":
			s.Origins, err = readSet(r)
		case "memory_bytes":
			s.MemoryBytes, err = readUint[uint64](r)
		case "gpu_bytes":
			s.GPUBytes, err = readUint[uint64](r)
		}
		return err
	})
}

// PackageOffer is a Rust manifest::PackageOffer: one offered .cxb.
type PackageOffer struct {
	ID           string
	PublisherKey string
	Digest       string
	Bytes        uint64
	URL          string
}

var packageOfferFields = []string{"id", "publisher_key", "digest", "bytes", "url"}

func (p PackageOffer) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: packageOfferFields}
	o.str(p.ID)
	o.str(p.PublisherKey)
	o.str(p.Digest)
	o.uint(p.Bytes)
	o.str(p.URL)
	return o.end()
}

func (p *PackageOffer) decodeJSON(r *reader) error {
	*p = PackageOffer{}
	return r.fields(packageOfferFields, func(name string) (err error) {
		switch name {
		case "id":
			p.ID, err = r.str()
		case "publisher_key":
			p.PublisherKey, err = r.str()
		case "digest":
			p.Digest, err = r.str()
		case "bytes":
			p.Bytes, err = readUint[uint64](r)
		case "url":
			p.URL, err = r.str()
		}
		return err
	})
}

// Offer is a Rust manifest::Offer, the deployment advertisement signed under OfferDomain.
type Offer struct {
	Version     uint16
	Audience    string
	ServerKey   string
	Revision    uint64
	ExpiresUnix uint64
	Scope       Scope
	Packages    []PackageOffer
	Fallback    string
	Carrier     string
}

var offerFields = []string{"version", "audience", "server_key", "revision", "expires_unix", "scope", "packages", "fallback", "carrier"}

func (f Offer) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: offerFields}
	o.uint(uint64(f.Version))
	o.str(f.Audience)
	o.str(f.ServerKey)
	o.uint(f.Revision)
	o.uint(f.ExpiresUnix)
	o.value(f.Scope)
	o.value(list[PackageOffer](f.Packages))
	o.str(f.Fallback)
	o.str(f.Carrier)
	return o.end()
}

func (f *Offer) decodeJSON(r *reader) error {
	*f = Offer{}
	return r.fields(offerFields, func(name string) (err error) {
		switch name {
		case "version":
			f.Version, err = readUint[uint16](r)
		case "audience":
			f.Audience, err = r.str()
		case "server_key":
			f.ServerKey, err = r.str()
		case "revision":
			f.Revision, err = readUint[uint64](r)
		case "expires_unix":
			f.ExpiresUnix, err = readUint[uint64](r)
		case "scope":
			err = f.Scope.decodeJSON(r)
		case "packages":
			f.Packages, err = readList[PackageOffer](r)
		case "fallback":
			f.Fallback, err = r.str()
		case "carrier":
			f.Carrier, err = r.str()
		}
		return err
	})
}

// SignedDocument is a Rust crypto::SignedDocument: lowercase hex of canonical payload bytes and
// of the Ed25519 signature over the domain followed by those bytes.
type SignedDocument struct {
	Payload   string
	Signature string
}

var signedDocumentFields = []string{"payload", "signature"}

func (d SignedDocument) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: signedDocumentFields}
	o.str(d.Payload)
	o.str(d.Signature)
	return o.end()
}

func (d *SignedDocument) decodeJSON(r *reader) error {
	*d = SignedDocument{}
	return r.fields(signedDocumentFields, func(name string) (err error) {
		switch name {
		case "payload":
			d.Payload, err = r.str()
		case "signature":
			d.Signature, err = r.str()
		}
		return err
	})
}

// Marker is a Rust negotiation::Marker, the file at MarkerPath in the offer's resource pack.
type Marker struct {
	ServerKey string
	Offer     SignedDocument
}

var markerFields = []string{"server_key", "offer"}

func (m Marker) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: markerFields}
	o.str(m.ServerKey)
	o.value(m.Offer)
	return o.end()
}

func (m *Marker) decodeJSON(r *reader) error {
	*m = Marker{}
	return r.fields(markerFields, func(name string) (err error) {
		switch name {
		case "server_key":
			m.ServerKey, err = r.str()
		case "offer":
			err = m.Offer.decodeJSON(r)
		}
		return err
	})
}

// Limits is a Rust negotiation::Limits, the ceilings of a wire version. A Hello offers the
// client's; an Accept may lower each and never raise one.
type Limits struct {
	// MaxFragmentBytes bounds an inline record and the data of one fragment.
	MaxFragmentBytes uint32
	// MaxMessageBytes bounds a reassembled record.
	MaxMessageBytes uint32
	// MaxReassemblyBytes bounds the carrier bytes one direction holds for undelivered messages,
	// an open fragmented one included.
	MaxReassemblyBytes uint32
}

// HostLimits are the ceilings this server offers, which equal the client's.
var HostLimits = Limits{MaxPayloadBytes, MaxMessageBytes, MaxQueueBytes}

// v1Limits are wire v1's fixed ceilings: whole envelopes of at most MaxPayloadBytes.
var v1Limits = Limits{MaxPayloadBytes, MaxPayloadBytes, MaxQueueBytes}

// lowest is each ceiling of l no higher than other's.
func (l Limits) lowest(other Limits) Limits {
	return Limits{
		min(l.MaxFragmentBytes, other.MaxFragmentBytes),
		min(l.MaxMessageBytes, other.MaxMessageBytes),
		min(l.MaxReassemblyBytes, other.MaxReassemblyBytes),
	}
}

// ordered reports what the client checks besides the offer: nonzero and each ceiling within
// the next.
func (l Limits) ordered() bool {
	return 0 < l.MaxFragmentBytes && l.MaxFragmentBytes <= l.MaxMessageBytes && l.MaxMessageBytes <= l.MaxReassemblyBytes
}

var limitsFields = []string{"max_fragment_bytes", "max_message_bytes", "max_reassembly_bytes"}

func (l Limits) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: limitsFields}
	o.uint(uint64(l.MaxFragmentBytes))
	o.uint(uint64(l.MaxMessageBytes))
	o.uint(uint64(l.MaxReassemblyBytes))
	return o.end()
}

func (l *Limits) decodeJSON(r *reader) error {
	*l = Limits{}
	return r.fields(limitsFields, func(name string) (err error) {
		switch name {
		case "max_fragment_bytes":
			l.MaxFragmentBytes, err = readUint[uint32](r)
		case "max_message_bytes":
			l.MaxMessageBytes, err = readUint[uint32](r)
		case "max_reassembly_bytes":
			l.MaxReassemblyBytes, err = readUint[uint32](r)
		}
		return err
	})
}

// WireOffer is a Rust negotiation::WireOffer: every wire version a v2 client speaks, a set that
// encodes ascending, and its ceilings.
type WireOffer struct {
	Versions []uint16
	Limits   Limits
}

var wireOfferFields = []string{"versions", "limits"}

func (w WireOffer) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: wireOfferFields}
	o.key()
	versions := slices.Compact(slices.Sorted(slices.Values(w.Versions)))
	o.b = append(o.b, '[')
	for i, v := range versions {
		if i > 0 {
			o.b = append(o.b, ',')
		}
		o.b = strconv.AppendUint(o.b, uint64(v), 10)
	}
	o.b = append(o.b, ']')
	o.value(w.Limits)
	return o.end()
}

func (w *WireOffer) decodeJSON(r *reader) error {
	*w = WireOffer{}
	return r.fields(wireOfferFields, func(name string) (err error) {
		switch name {
		case "versions":
			// A BTreeSet<u16>: any order, repeats merged, held ascending.
			err = r.array(func() error {
				v, err := readUint[uint16](r)
				w.Versions = append(w.Versions, v)
				return err
			})
			w.Versions = slices.Compact(slices.Sorted(slices.Values(w.Versions)))
		case "limits":
			err = w.Limits.decodeJSON(r)
		}
		return err
	})
}

// Wire is a Rust negotiation::Wire: the version and ceilings that an Accept selects.
type Wire struct {
	Version uint16
	Limits  Limits
}

// v1Wire is the session of a v1 Accept, which selects nothing.
var v1Wire = Wire{WireVersion, v1Limits}

var wireFields = []string{"version", "limits"}

func (w Wire) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: wireFields}
	o.uint(uint64(w.Version))
	o.value(w.Limits)
	return o.end()
}

func (w *Wire) decodeJSON(r *reader) error {
	*w = Wire{}
	return r.fields(wireFields, func(name string) (err error) {
		switch name {
		case "version":
			w.Version, err = readUint[uint16](r)
		case "limits":
			err = w.Limits.decodeJSON(r)
		}
		return err
	})
}

// Hello is a Rust negotiation::Hello, the client's first handshake message. Wire is nil in a v1
// client's Hello.
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

func (h Hello) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: helloFields}
	o.uint(uint64(h.Version))
	o.uint(uint64(h.API))
	o.value(h.Capabilities)
	o.str(h.OfferDigest)
	o.str(h.ClientChallenge)
	o.str(h.Connection)
	o.uint(uint64(h.Subclient))
	if h.Wire != nil {
		o.value(*h.Wire)
	} else {
		o.skip()
	}
	return o.end()
}

func (h *Hello) decodeJSON(r *reader) error {
	*h = Hello{}
	return r.fields(helloFields, func(name string) (err error) {
		switch name {
		case "version":
			h.Version, err = readUint[uint16](r)
		case "api":
			h.API, err = readUint[uint16](r)
		case "capabilities":
			err = h.Capabilities.decodeJSON(r)
		case "offer_digest":
			h.OfferDigest, err = r.str()
		case "client_challenge":
			h.ClientChallenge, err = r.str()
		case "connection":
			h.Connection, err = r.str()
		case "subclient":
			h.Subclient, err = readUint[uint8](r)
		case "wire":
			if !r.null() {
				h.Wire = new(WireOffer)
				err = h.Wire.decodeJSON(r)
			}
		}
		return err
	}, "wire")
}

// Accept is a Rust negotiation::Accept, signed under AcceptDomain by the offer's server key.
// Hello is the client's Hello, echoed. Wire is nil when the server selects v1.
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

func (a Accept) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: acceptFields}
	o.value(a.Hello)
	o.str(a.ServerChallenge)
	o.str(a.Session)
	o.str(a.Audience)
	o.str(a.OfferDigest)
	o.uint(a.Revision)
	o.uint(a.ExpiresUnix)
	if a.Wire != nil {
		o.value(*a.Wire)
	} else {
		o.skip()
	}
	return o.end()
}

func (a *Accept) decodeJSON(r *reader) error {
	*a = Accept{}
	return r.fields(acceptFields, func(name string) (err error) {
		switch name {
		case "hello":
			err = a.Hello.decodeJSON(r)
		case "server_challenge":
			a.ServerChallenge, err = r.str()
		case "session":
			a.Session, err = r.str()
		case "audience":
			a.Audience, err = r.str()
		case "offer_digest":
			a.OfferDigest, err = r.str()
		case "revision":
			a.Revision, err = readUint[uint64](r)
		case "expires_unix":
			a.ExpiresUnix, err = readUint[uint64](r)
		case "wire":
			if !r.null() {
				a.Wire = new(Wire)
				err = a.Wire.decodeJSON(r)
			}
		}
		return err
	}, "wire")
}

// Ready is the body of Rust's Control::Ready: the client's runtime is up. Packages are the bundle
// digests in offer order; Permissions maps each bundle id to its granted permissions.
type Ready struct {
	Session     string
	Packages    []string
	Generation  uint64
	Permissions map[string]Permissions
	WorldEpoch  uint64
}

var readyFields = []string{"session", "packages", "generation", "permissions", "world_epoch"}

func (y Ready) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: readyFields}
	o.str(y.Session)
	o.key()
	o.add(appendStrings(o.b, y.Packages))
	o.uint(y.Generation)
	o.key()
	o.add(appendPermissionMap(o.b, y.Permissions))
	o.uint(y.WorldEpoch)
	return o.end()
}

// appendPermissionMap appends m as Rust serializes a BTreeMap<String, BTreeSet<Permission>>: in
// key order.
func appendPermissionMap(b []byte, m map[string]Permissions) ([]byte, error) {
	b = append(b, '{')
	for i, key := range slices.Sorted(maps.Keys(m)) {
		if i > 0 {
			b = append(b, ',')
		}
		var err error
		if b, err = appendString(b, key); err != nil {
			return nil, err
		}
		b = append(b, ':')
		if b, err = m[key].appendJSON(b); err != nil {
			return nil, err
		}
	}
	return append(b, '}'), nil
}

func (y *Ready) decodeJSON(r *reader) error {
	*y = Ready{}
	return r.fields(readyFields, func(name string) (err error) {
		switch name {
		case "session":
			y.Session, err = r.str()
		case "packages":
			y.Packages, err = readStrings(r)
		case "generation":
			y.Generation, err = readUint[uint64](r)
		case "permissions":
			// A repeated key replaces the earlier one, as BTreeMap::insert does.
			y.Permissions = map[string]Permissions{}
			err = r.object(func(key string) error {
				var set Permissions
				err := set.decodeJSON(r)
				y.Permissions[key] = set
				return err
			})
		case "world_epoch":
			y.WorldEpoch, err = readUint[uint64](r)
		}
		return err
	})
}

// Epoch is the body of Rust's Control::Epoch: on wire v2 a client whose world epoch changed
// keeps its runtime and names its session and new epoch.
type Epoch struct {
	Session    string
	WorldEpoch uint64
}

var epochFields = []string{"session", "world_epoch"}

func (e Epoch) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: epochFields}
	o.str(e.Session)
	o.uint(e.WorldEpoch)
	return o.end()
}

func (e *Epoch) decodeJSON(r *reader) error {
	*e = Epoch{}
	return r.fields(epochFields, func(name string) (err error) {
		switch name {
		case "session":
			e.Session, err = r.str()
		case "world_epoch":
			e.WorldEpoch, err = readUint[uint64](r)
		}
		return err
	})
}

// Control is a Rust session::Control, a handshake message on the carrier: {"kind":…,"body":…}.
// Exactly one field is set.
type Control struct {
	Hello  *Hello
	Accept *SignedDocument
	Ready  *Ready
	Epoch  *Epoch
	// Disabled is reserved: the client never sends it and revocation never relies on it.
	Disabled bool
}

var controlFields = []string{"kind", "body"}

func (c Control) appendJSON(b []byte) ([]byte, error) {
	var kind string
	var body Message
	variants := 0
	if c.Hello != nil {
		kind, body = "hello", *c.Hello
		variants++
	}
	if c.Accept != nil {
		kind, body = "accept", *c.Accept
		variants++
	}
	if c.Ready != nil {
		kind, body = "ready", *c.Ready
		variants++
	}
	if c.Epoch != nil {
		kind, body = "epoch", *c.Epoch
		variants++
	}
	if c.Disabled {
		kind, body = "disabled", nil
		variants++
	}
	if variants != 1 {
		return nil, fmt.Errorf("a control message holds %d variants, not 1", variants)
	}
	b = append(b, `{"kind":"`...)
	b = append(b, kind...)
	b = append(b, '"')
	if body != nil {
		b = append(b, `,"body":`...)
		var err error
		if b, err = body.appendJSON(b); err != nil {
			return nil, err
		}
	}
	return append(b, '}'), nil
}

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
	case "disabled":
		c.Disabled = true
	default:
		return r.errorf("unknown control kind %q", kind)
	}
	return within(raw, func(sub *reader) error {
		member := func(name string) error {
			switch {
			case name == "kind":
				_, err := sub.str()
				return err
			case body != nil:
				return body.decodeJSON(sub)
			case sub.null():
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

// ContentFile is a Rust manifest::ContentFile, one indexed file of a bundle.
type ContentFile struct {
	Path   string
	Bytes  uint64
	SHA256 string
}

var contentFileFields = []string{"path", "bytes", "sha256"}

func (f ContentFile) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: contentFileFields}
	o.str(f.Path)
	o.uint(f.Bytes)
	o.str(f.SHA256)
	return o.end()
}

func (f *ContentFile) decodeJSON(r *reader) error {
	*f = ContentFile{}
	return r.fields(contentFileFields, func(name string) (err error) {
		switch name {
		case "path":
			f.Path, err = r.str()
		case "bytes":
			f.Bytes, err = readUint[uint64](r)
		case "sha256":
			f.SHA256, err = r.str()
		}
		return err
	})
}

// Manifest is a Rust manifest::Manifest, the bundle index signed under ManifestDomain by the
// publisher key.
type Manifest struct {
	Version        uint16
	API            uint16
	ID             string
	PublisherKey   string
	PackageVersion string
	Permissions    Permissions
	// Component is the path of the client component, or nil (null) for a bundle without one.
	Component *string
	Channels  []Channel
	// Actions is a set of identifiers; it encodes sorted.
	Actions []string
	// Templates is the set of indexed JSON-UI template paths; it encodes sorted, and not at all
	// when empty.
	Templates []string
	Files     []ContentFile
}

var manifestFields = []string{"version", "api", "id", "publisher_key", "package_version", "permissions", "component", "channels", "actions", "templates", "files"}

func (m Manifest) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: manifestFields}
	o.uint(uint64(m.Version))
	o.uint(uint64(m.API))
	o.str(m.ID)
	o.str(m.PublisherKey)
	o.str(m.PackageVersion)
	o.value(m.Permissions)
	if m.Component != nil {
		o.str(*m.Component)
	} else {
		o.key()
		o.b = append(o.b, "null"...)
	}
	o.value(list[Channel](m.Channels))
	o.key()
	o.add(appendSet(o.b, m.Actions))
	if len(m.Templates) > 0 {
		o.key()
		o.add(appendSet(o.b, m.Templates))
	} else {
		o.skip()
	}
	o.value(list[ContentFile](m.Files))
	return o.end()
}

func (m *Manifest) decodeJSON(r *reader) error {
	*m = Manifest{}
	return r.fields(manifestFields, func(name string) (err error) {
		switch name {
		case "version":
			m.Version, err = readUint[uint16](r)
		case "api":
			m.API, err = readUint[uint16](r)
		case "id":
			m.ID, err = r.str()
		case "publisher_key":
			m.PublisherKey, err = r.str()
		case "package_version":
			m.PackageVersion, err = r.str()
		case "permissions":
			err = m.Permissions.decodeJSON(r)
		case "component":
			if !r.null() {
				var component string
				component, err = r.str()
				m.Component = &component
			}
		case "channels":
			m.Channels, err = readList[Channel](r)
		case "actions":
			m.Actions, err = readSet(r)
		case "templates":
			m.Templates, err = readSet(r)
		case "files":
			m.Files, err = readList[ContentFile](r)
		}
		return err
	}, "component", "templates")
}
