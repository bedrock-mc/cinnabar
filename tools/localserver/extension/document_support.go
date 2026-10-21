package extension

import (
	"fmt"
	"maps"
	"slices"
	"strconv"
)

// Permissions is a Rust BTreeSet<Permission>: one bit per Permission. It encodes in declaration
// order, which is the set's order.
type Permissions uint64

// The highest declared permission must fit in the set storage.
const _ Permissions = 1 << (len(permissionNames) - 1)

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
		return nil, fmt.Errorf("permissions %#x hold a permission Rust does not have", uint64(s))
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

// HostLimits are the ceilings this server offers, which equal the client's.
var HostLimits = Limits{MaxPayloadBytes, MaxMessageBytes, MaxQueueBytes}

// v1Limits are wire v1's fixed ceilings: whole envelopes of at most MaxPayloadBytes.
var v1Limits = Limits{MaxPayloadBytes, MaxPayloadBytes, MaxQueueBytes}

// v1Wire is the session of a v1 Accept, which selects nothing.
var v1Wire = Wire{WireVersion, v1Limits}

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

// appendVersions writes a sorted, duplicate-free version set.
func appendVersions(b []byte, versions []uint16) ([]byte, error) {
	b = append(b, '[')
	for i, v := range slices.Compact(slices.Sorted(slices.Values(versions))) {
		if i > 0 {
			b = append(b, ',')
		}
		b = strconv.AppendUint(b, uint64(v), 10)
	}
	return append(b, ']'), nil
}

// readVersions reads the version set in ascending order.
func readVersions(r *reader) ([]uint16, error) {
	var values []uint16
	err := r.array(func() error { v, err := readUint[uint16](r); values = append(values, v); return err })
	return slices.Compact(slices.Sorted(slices.Values(values))), err
}

// readPermissionMap uses last-key-wins semantics, matching BTreeMap.
func readPermissionMap(r *reader) (map[string]Permissions, error) {
	values := map[string]Permissions{}
	err := r.object(func(key string) error { var set Permissions; err := set.decodeJSON(r); values[key] = set; return err })
	return values, err
}
