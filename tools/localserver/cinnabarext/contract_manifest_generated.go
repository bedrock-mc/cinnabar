// Generated from crates/server-experience/schema/extension.json; do not edit.
package cinnabarext

import (
	"fmt"
	"slices"
	"strings"
)

// Scope is the schema-defined Scope document.
type Scope struct {
	Permissions  Permissions
	Origins      []string
	MemoryBytes  uint64
	GPUBytes     uint64
	PersonalMods []string
}

var scopeFields = []string{"permissions", "origins", "memory_bytes", "gpu_bytes", "personal_mods"}

// appendJSON writes Scope in canonical field order.
func (v Scope) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: scopeFields}
	o.value(v.Permissions)
	o.key()
	o.add(appendSet(o.b, v.Origins))
	o.uint(uint64(v.MemoryBytes))
	o.uint(uint64(v.GPUBytes))
	if len(v.PersonalMods) > 0 {
		o.key()
		o.add(appendSet(o.b, v.PersonalMods))
	} else {
		o.skip()
	}
	return o.end()
}

// decodeJSON reads a strict Scope object.
func (v *Scope) decodeJSON(r *reader) error {
	*v = Scope{}
	return r.fields(scopeFields, func(name string) (err error) {
		switch name {
		case "permissions":
			err = v.Permissions.decodeJSON(r)
		case "origins":
			v.Origins, err = readSet(r)
		case "memory_bytes":
			v.MemoryBytes, err = readUint[uint64](r)
		case "gpu_bytes":
			v.GPUBytes, err = readUint[uint64](r)
		case "personal_mods":
			v.PersonalMods, err = readSet(r)
		}
		return err
	}, "personal_mods")
}

// PackageOffer is the schema-defined PackageOffer document.
type PackageOffer struct {
	ID           string
	PublisherKey string
	Digest       string
	Bytes        uint64
	URL          string
}

var packageOfferFields = []string{"id", "publisher_key", "digest", "bytes", "url"}

// appendJSON writes PackageOffer in canonical field order.
func (v PackageOffer) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: packageOfferFields}
	o.str(v.ID)
	o.str(v.PublisherKey)
	o.str(v.Digest)
	o.uint(uint64(v.Bytes))
	o.str(v.URL)
	return o.end()
}

// decodeJSON reads a strict PackageOffer object.
func (v *PackageOffer) decodeJSON(r *reader) error {
	*v = PackageOffer{}
	return r.fields(packageOfferFields, func(name string) (err error) {
		switch name {
		case "id":
			v.ID, err = r.str()
		case "publisher_key":
			v.PublisherKey, err = r.str()
		case "digest":
			v.Digest, err = r.str()
		case "bytes":
			v.Bytes, err = readUint[uint64](r)
		case "url":
			v.URL, err = r.str()
		}
		return err
	})
}

// Offer is the schema-defined Offer document.
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

// appendJSON writes Offer in canonical field order.
func (v Offer) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: offerFields}
	o.uint(uint64(v.Version))
	o.str(v.Audience)
	o.str(v.ServerKey)
	o.uint(uint64(v.Revision))
	o.uint(uint64(v.ExpiresUnix))
	o.value(v.Scope)
	o.value(list[PackageOffer](v.Packages))
	o.str(v.Fallback)
	o.str(v.Carrier)
	return o.end()
}

// decodeJSON reads a strict Offer object.
func (v *Offer) decodeJSON(r *reader) error {
	*v = Offer{}
	return r.fields(offerFields, func(name string) (err error) {
		switch name {
		case "version":
			v.Version, err = readUint[uint16](r)
		case "audience":
			v.Audience, err = r.str()
		case "server_key":
			v.ServerKey, err = r.str()
		case "revision":
			v.Revision, err = readUint[uint64](r)
		case "expires_unix":
			v.ExpiresUnix, err = readUint[uint64](r)
		case "scope":
			err = v.Scope.decodeJSON(r)
		case "packages":
			v.Packages, err = readList[PackageOffer](r)
		case "fallback":
			v.Fallback, err = r.str()
		case "carrier":
			v.Carrier, err = r.str()
		}
		return err
	})
}

// ContentFile is the schema-defined ContentFile document.
type ContentFile struct {
	Path   string
	Bytes  uint64
	SHA256 string
}

var contentFileFields = []string{"path", "bytes", "sha256"}

// appendJSON writes ContentFile in canonical field order.
func (v ContentFile) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: contentFileFields}
	o.str(v.Path)
	o.uint(uint64(v.Bytes))
	o.str(v.SHA256)
	return o.end()
}

// decodeJSON reads a strict ContentFile object.
func (v *ContentFile) decodeJSON(r *reader) error {
	*v = ContentFile{}
	return r.fields(contentFileFields, func(name string) (err error) {
		switch name {
		case "path":
			v.Path, err = r.str()
		case "bytes":
			v.Bytes, err = readUint[uint64](r)
		case "sha256":
			v.SHA256, err = r.str()
		}
		return err
	})
}

// Manifest is the schema-defined Manifest document.
type Manifest struct {
	Version        uint16
	API            uint16
	ID             string
	PublisherKey   string
	PackageVersion string
	Permissions    Permissions
	Component      *string
	Channels       []Channel
	Actions        []string
	Templates      []string
	Files          []ContentFile
}

var manifestFields = []string{"version", "api", "id", "publisher_key", "package_version", "permissions", "component", "channels", "actions", "templates", "files"}

// appendJSON writes Manifest in canonical field order.
func (v Manifest) appendJSON(b []byte) ([]byte, error) {
	o := object{b: b, names: manifestFields}
	o.uint(uint64(v.Version))
	o.uint(uint64(v.API))
	o.str(v.ID)
	o.str(v.PublisherKey)
	o.str(v.PackageVersion)
	o.value(v.Permissions)
	if v.Component != nil {
		o.str(*v.Component)
	} else {
		o.key()
		o.b = append(o.b, "null"...)
	}
	o.value(list[Channel](v.Channels))
	o.key()
	o.add(appendSet(o.b, v.Actions))
	if len(v.Templates) > 0 {
		o.key()
		o.add(appendSet(o.b, v.Templates))
	} else {
		o.skip()
	}
	o.value(list[ContentFile](v.Files))
	return o.end()
}

// decodeJSON reads a strict Manifest object.
func (v *Manifest) decodeJSON(r *reader) error {
	*v = Manifest{}
	return r.fields(manifestFields, func(name string) (err error) {
		switch name {
		case "version":
			v.Version, err = readUint[uint16](r)
		case "api":
			v.API, err = readUint[uint16](r)
		case "id":
			v.ID, err = r.str()
		case "publisher_key":
			v.PublisherKey, err = r.str()
		case "package_version":
			v.PackageVersion, err = r.str()
		case "permissions":
			err = v.Permissions.decodeJSON(r)
		case "component":
			if !r.null() {
				var item string
				item, err = r.str()
				v.Component = &item
			}
		case "channels":
			v.Channels, err = readList[Channel](r)
		case "actions":
			v.Actions, err = readSet(r)
		case "templates":
			v.Templates, err = readSet(r)
		case "files":
			v.Files, err = readList[ContentFile](r)
		}
		return err
	}, "component", "templates")
}

type Permission uint8

const (
	PermissionUI Permission = iota
	PermissionModalUI
	PermissionInput
	PermissionMessaging
	PermissionScene
	PermissionMedia
	PermissionItems
	PermissionWorldDraw
	PermissionEntityObserve
	PermissionEntityHighlight
	PermissionRenderThroughWalls
	PermissionRenderMaterial
	PermissionRenderPost
)

var permissionNames = [...]string{"ui", "modal_ui", "input", "messaging", "scene", "media", "items", "world_draw", "entity_observe", "entity_highlight", "render_through_walls", "render_material", "render_post"}

// String returns the schema name, or a diagnostic for an invalid enum.
func (v Permission) String() string {
	if int(v) < len(permissionNames) {
		return permissionNames[v]
	}
	return fmt.Sprintf("Permission(%d)", uint8(v))
}

// appendJSON encodes one declared enum name.
func (v Permission) appendJSON(b []byte) ([]byte, error) {
	if int(v) >= len(permissionNames) {
		return nil, fmt.Errorf("invalid Permission %d", v)
	}
	return appendString(b, permissionNames[v])
}

// decodeJSON rejects names outside the schema.
func (v *Permission) decodeJSON(r *reader) error {
	name, err := r.str()
	if err != nil {
		return err
	}
	i := slices.Index(permissionNames[:], name)
	if i < 0 {
		return r.errorf("unknown Permission %q", name)
	}
	*v = Permission(i)
	return nil
}

// Identifier restricts names to the shared portable namespace.
func Identifier(text string) bool {
	if text == "" || len(text) > MaxIdentifierBytes {
		return false
	}
	for i := range len(text) {
		c := text[i]
		if !(c >= 'a' && c <= 'z' || c >= '0' && c <= '9' || strings.IndexByte(":_-.", c) >= 0) {
			return false
		}
	}
	return true
}

const ImplementedPermissions = Permissions(1<<PermissionUI | 1<<PermissionModalUI | 1<<PermissionInput | 1<<PermissionMessaging | 1<<PermissionWorldDraw | 1<<PermissionEntityObserve | 1<<PermissionEntityHighlight | 1<<PermissionRenderThroughWalls | 1<<PermissionRenderMaterial | 1<<PermissionRenderPost)
