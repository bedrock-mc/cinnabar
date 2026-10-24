package experience

import (
	"cmp"
	"encoding/hex"
	"errors"
	"fmt"
	"slices"

	"github.com/google/uuid"
)

// stateAddress is a mod-private scope. Only player UUIDs and stable instance IDs reach Owner.
type stateAddress struct {
	Scope StoreScope
	Owner string
}

// stateEntry retains a revision even after deletion, so absent/present/absent cannot hide a write.
type stateEntry struct {
	Schema   uint32
	Revision uint64
	Data     []byte
}

// storedState is one persisted private record, sharing the block store's file and quota.
type storedState struct {
	Scope    StoreScope `json:"scope"`
	Owner    string     `json:"owner"`
	Schema   uint32     `json:"schema"`
	Revision uint64     `json:"revision"`
	Data     *string    `json:"data"`
}

// validStateAddress rejects ambiguous scope identities when loading state from disk.
func validStateAddress(a stateAddress) bool {
	switch a.Scope {
	case StoreMod:
		return a.Owner == ""
	case StoreInstance:
		return a.Owner != ""
	case StorePlayer:
		id, err := uuid.Parse(a.Owner)
		return err == nil && id.String() == a.Owner
	}
	return false
}

// stateAddresses gives a callback only its actor, current instance and own mod record.
func stateAddresses(instance WorldInstanceID, actor *string) []stateAddress {
	addresses := []stateAddress{{Scope: StoreInstance, Owner: string(instance)}, {Scope: StoreMod}}
	if actor != nil {
		addresses = append(addresses, stateAddress{Scope: StorePlayer, Owner: *actor})
	}
	return addresses
}

// readStates takes all authorized records and their revisions under the existing store lock.
func (s *Store) readStates(exp string, addresses []stateAddress) []StateRecord {
	s.mu.Lock()
	defer s.mu.Unlock()
	out := make([]StateRecord, 0, len(addresses))
	for _, a := range addresses {
		var entry stateEntry
		if e := s.experience(exp, false); e != nil {
			entry = e.states[a]
		}
		record := StateRecord{Key: StoreKey{Scope: a.Scope, Schema: entry.Schema}, Revision: entry.Revision}
		if entry.Data != nil {
			data := hex.EncodeToString(entry.Data)
			record.Data = &data
		}
		out = append(out, record)
	}
	return out
}

// statesCurrent rejects a result if any private record it could read has changed.
func (s *Store) statesCurrent(exp string, addresses []stateAddress, records []StateRecord) bool {
	return slices.EqualFunc(s.readStates(exp, addresses), records, func(a, b StateRecord) bool {
		return a.Key == b.Key && a.Revision == b.Revision
	})
}

// setState replaces one validated record. It never aliases the caller's byte slice.
func (s *Store) setState(exp string, address stateAddress, key StoreKey, data []byte, present bool) error {
	if !validStateAddress(address) || address.Scope != key.Scope {
		return errors.New("invalid state scope")
	}
	if len(data) > maxBlockDataBytes {
		return ErrQuota
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	e := s.experience(exp, true)
	if e.states == nil {
		e.states = make(map[stateAddress]stateEntry)
	}
	old := e.states[address]
	var stored []byte
	if present {
		stored = append(make([]byte, 0, len(data)), data...)
	}
	used := e.used - uint64(len(old.Data)) + uint64(len(stored))
	if used > dataQuota {
		return ErrQuota
	}
	e.states[address] = stateEntry{Schema: key.Schema, Revision: old.Revision + 1, Data: stored}
	e.used, e.dirty = used, true
	return nil
}

// storedStates snapshots immutable byte slices for Flush and orders records deterministically.
func storedStates(states map[stateAddress]stateEntry) []storedState {
	out := make([]storedState, 0, len(states))
	for address, entry := range states {
		record := storedState{Scope: address.Scope, Owner: address.Owner, Schema: entry.Schema, Revision: entry.Revision}
		if entry.Data != nil {
			data := hex.EncodeToString(entry.Data)
			record.Data = &data
		}
		out = append(out, record)
	}
	slices.SortFunc(out, func(a, b storedState) int {
		return cmp.Or(cmp.Compare(a.Scope, b.Scope), cmp.Compare(a.Owner, b.Owner))
	})
	return out
}

// stateWrite is a validated scoped replacement and its net quota change.
type stateWrite struct {
	address stateAddress
	key     StoreKey
	data    []byte
	present bool
	delta   int64
}

// validateStateWrite rechecks authority, unique scopes, byte limits, and snapshotted state.
func (h *Host) validateStateWrite(ev event, snap snapshot, op *StoreWriteOp, earlier []stateWrite) (stateWrite, error) {
	context := snap.req.Info.Context
	if context == nil {
		return stateWrite{}, errInvalid
	}
	for _, write := range earlier {
		if write.key.Scope == op.Key.Scope {
			return stateWrite{}, errInvalid
		}
	}
	var address stateAddress
	authorized := false
	for _, a := range stateAddresses(ev.instance.handle.ID, ev.storePrincipal(snap.req.Actor)) {
		if a.Scope == op.Key.Scope {
			address, authorized = a, true
		}
	}
	if !authorized {
		return stateWrite{}, fmt.Errorf("%w: private store scope denied", errInvalid)
	}
	write := stateWrite{address: address, key: op.Key, present: op.Data != nil}
	var err error
	if op.Data != nil {
		write.data, err = hex.DecodeString(*op.Data)
		if err != nil || len(write.data) > maxBlockDataBytes {
			return stateWrite{}, errInvalid
		}
	}
	old := 0
	for _, record := range context.Stores {
		if record.Key.Scope == op.Key.Scope && record.Data != nil {
			old = len(*record.Data) / 2
		}
	}
	write.delta = int64(len(write.data)) - int64(old)
	return write, nil
}
