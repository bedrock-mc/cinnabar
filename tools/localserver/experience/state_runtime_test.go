package experience

import (
	"errors"
	"testing"

	"github.com/df-mc/dragonfly/server/world"
)

// TestScopedStoresPersistIsolatedRecords covers mod namespaces, players, instances and tombstones.
func TestScopedStoresPersistIsolatedRecords(t *testing.T) {
	dir := t.TempDir()
	s := openTestStore(t, dir)
	first := stateAddress{Scope: StoreInstance, Owner: "probe:one"}
	second := stateAddress{Scope: StoreInstance, Owner: "probe:two"}
	player := stateAddress{Scope: StorePlayer, Owner: "11111111-1111-1111-1111-111111111111"}
	mod := stateAddress{Scope: StoreMod}
	for i, address := range []stateAddress{first, second, player, mod} {
		if err := s.setState("probe", address, StoreKey{Scope: address.Scope, Schema: 3}, []byte{byte(i)}, true); err != nil {
			t.Fatal(err)
		}
	}
	if err := s.setState("other", first, StoreKey{Scope: StoreInstance, Schema: 9}, []byte{99}, true); err != nil {
		t.Fatal(err)
	}
	if err := s.Flush(); err != nil {
		t.Fatal(err)
	}
	s = openTestStore(t, dir)
	for i, address := range []stateAddress{first, second, player, mod} {
		record := s.readStates("probe", []stateAddress{address})[0]
		want := "0" + string("0123"[i])
		if record.Data == nil || *record.Data != want || record.Key.Schema != 3 || record.Revision != 1 {
			t.Fatalf("scope %v changed: %+v", address, record)
		}
	}
	was := s.readStates("probe", []stateAddress{first})
	if err := s.setState("probe", first, StoreKey{Scope: StoreInstance, Schema: 3}, nil, false); err != nil {
		t.Fatal(err)
	}
	if s.statesCurrent("probe", []stateAddress{first}, was) {
		t.Fatal("deleted record matched old revision")
	}
	if data := s.readStates("probe", []stateAddress{second})[0].Data; data == nil || *data != "01" {
		t.Fatal("instance deletion changed another instance")
	}
	if budget := s.Budget("probe"); budget != dataQuota-3 {
		t.Fatalf("state did not share quota: %d", budget)
	}
}

// TestScopedResultRejectsConcurrentStateChange discards every write when a state read is stale.
func TestScopedResultRejectsConcurrentStateChange(t *testing.T) {
	f := newRuntimeFixture(t)
	ev, snap := f.request()
	address := stateAddress{Scope: StoreMod}
	if err := f.h.store.setState("probe", address, StoreKey{Scope: StoreMod, Schema: 1}, []byte("new"), true); err != nil {
		t.Fatal(err)
	}
	data := "01"
	f.do(f.source, func(tx *world.Tx) {
		if _, err := f.h.apply(tx, "probe", ev, snap, []Op{{StoreWrite: &StoreWriteOp{Key: StoreKey{Scope: StoreInstance, Schema: 1}, Data: &data}}}); !errors.Is(err, errStale) {
			t.Fatalf("stale private read committed: %v", err)
		}
	})
	if got := f.h.store.readStates("probe", []stateAddress{{Scope: StoreInstance, Owner: string(ev.instance.handle.ID)}})[0]; got.Data != nil {
		t.Fatal("rejected result partially committed")
	}
}

// TestScopedWriteCommitsOnlyAuthorizedScopes checks the adapter independently of runtime checks.
func TestScopedWriteCommitsOnlyAuthorizedScopes(t *testing.T) {
	f := newRuntimeFixture(t)
	ev, snap := f.request()
	data := "abcd"
	f.do(f.source, func(tx *world.Tx) {
		_, err := f.h.apply(tx, "probe", ev, snap, []Op{{StoreWrite: &StoreWriteOp{Key: StoreKey{Scope: StoreInstance, Schema: 1}, Data: &data}}})
		if err != nil {
			t.Fatal(err)
		}
	})
	ev, snap = f.request()
	f.do(f.source, func(tx *world.Tx) {
		_, err := f.h.apply(tx, "probe", ev, snap, []Op{{StoreWrite: &StoreWriteOp{Key: StoreKey{Scope: "unrelated", Schema: 1}, Data: &data}}})
		if !errors.Is(err, errInvalid) {
			t.Fatalf("unauthorized scope committed: %v", err)
		}
	})
	if budget := f.h.store.Budget("probe"); budget != dataQuota-2 {
		t.Fatal("committed state not accounted")
	}
}
