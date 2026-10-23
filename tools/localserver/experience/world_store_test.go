package experience

import (
	"encoding/json"
	"errors"
	"log/slog"
	"os"
	"path/filepath"
	"testing"

	"github.com/df-mc/dragonfly/server/world"
)

// TestBreakInAnotherSameTypeWorldKeepsData covers two live worlds at identical coordinates.
func TestBreakInAnotherSameTypeWorldKeepsData(t *testing.T) {
	f := newIdleFixture(t, slog.New(slog.DiscardHandler), nil)
	pos := BlockPos{X: 1, Y: 100, Z: 40}
	f.place(probeCounter, pos, []byte("first world"))
	other := world.Config{Synchronous: true, Log: slog.New(slog.DiscardHandler)}.New()
	t.Cleanup(func() { _ = other.Close() })
	if _, err := f.host.BindWorld("second", other); err != nil {
		t.Fatal(err)
	}
	b, _ := f.host.reg.Lookup(probeCounter)
	if err := other.Do(func(tx *world.Tx) {
		tx.SetBlock(pos.cube(), b, nil)
		f.host.breakHandler(b, pos.cube(), tx, nil)
	}).Wait(t.Context()); err != nil {
		t.Fatal(err)
	}
	f.assertData("probe", pos, []byte("first world"))
}

// TestInstanceStoreRoundTrip keeps same-type instances separate across flush and reopen.
func TestInstanceStoreRoundTrip(t *testing.T) {
	dir := t.TempDir()
	s := openTestStore(t, dir)
	keys := []Key{{Instance: "first", X: 1, Y: 64}, {Instance: "second", X: 1, Y: 64}}
	for i, key := range keys {
		s.Place("probe", key)
		if err := s.SetData("probe", key, []byte{byte(i)}, true); err != nil {
			t.Fatal(err)
		}
	}
	if err := s.Flush(); err != nil {
		t.Fatal(err)
	}
	r := openTestStore(t, dir)
	for i, key := range keys {
		data, ok := r.Data("probe", key)
		if !ok || len(data) != 1 || data[0] != byte(i) {
			t.Fatalf("reopened %q: data %x, present %v", key.Instance, data, ok)
		}
	}
	r.Remove("probe", keys[1])
	if data, ok := r.Data("probe", keys[0]); !ok || len(data) != 1 || data[0] != 0 {
		t.Fatal("removing the second instance changed the first")
	}
}

// TestStoredDefaultInstanceIDsStayReadable pins the identities already written to disk.
func TestStoredDefaultInstanceIDsStayReadable(t *testing.T) {
	raw, err := os.ReadFile(filepath.Join("testdata", "stores", "defaults-v2.json"))
	if err != nil {
		t.Fatal(err)
	}
	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, "probe.json"), raw, 0o600); err != nil {
		t.Fatal(err)
	}
	s := openTestStore(t, dir)
	for i, id := range []WorldInstanceID{OverworldInstance, NetherInstance, EndInstance} {
		key := Key{Instance: id, Y: 64}
		if data, ok := s.Data("probe", key); !ok || len(data) != 1 || data[0] != byte(i) {
			t.Fatalf("stored default instance %s no longer resolves: %x, %v", id, data, ok)
		}
	}
}

// TestLegacyStoreMigration preserves data, absence, placement generations and revisions.
func TestLegacyStoreMigration(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "probe.json")
	legacy := `{"schema":1,"next_generation":9,"entries":[
		{"dim":0,"x":1,"y":64,"z":0,"generation":3,"revision":4,"data":"0102"},
		{"dim":1,"x":1,"y":64,"z":0,"generation":5,"revision":6,"data":""},
		{"dim":2,"x":1,"y":64,"z":0,"generation":7,"revision":8,"data":null}]}`
	if err := os.WriteFile(path, []byte(legacy), 0o600); err != nil {
		t.Fatal(err)
	}
	s := openTestStore(t, dir)
	check := func(s *Store) {
		t.Helper()
		for i, instance := range []WorldInstanceID{OverworldInstance, NetherInstance, EndInstance} {
			key := Key{Instance: instance, X: 1, Y: 64}
			want := Token{Generation: uint64(3 + i*2), Revision: uint64(4 + i*2)}
			if token, ok := s.Token("probe", key); !ok || token != want {
				t.Fatalf("%s: token %v, present %v, want %v", instance, token, ok, want)
			}
			data, present := s.Data("probe", key)
			if present != (i < 2) || (i == 0 && (len(data) != 2 || data[0] != 1 || data[1] != 2)) || (i > 0 && len(data) != 0) {
				t.Fatalf("%s: data %x, present %v", instance, data, present)
			}
		}
		if budget := s.Budget("probe"); budget != dataQuota-2 {
			t.Fatalf("migrated quota budget %d", budget)
		}
	}
	check(s)
	if err := s.Flush(); err != nil {
		t.Fatal(err)
	}
	check(openTestStore(t, dir))
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	var file experienceFile
	if err := json.Unmarshal(raw, &file); err != nil {
		t.Fatal(err)
	}
	if file.Schema != storeSchema {
		t.Fatalf("migration was not persisted: schema %d", file.Schema)
	}
	for _, entry := range file.Entries {
		if entry.Instance == "" || entry.LegacyDim != nil {
			t.Fatalf("migration left an ambiguous entry: %+v", entry)
		}
	}
	if token := s.Place("probe", Key{Instance: "another"}); token.Generation < 9 {
		t.Fatalf("migration reused generation %d", token.Generation)
	}
}

// TestWorldHandlesExpire rejects earlier opening handles without invalidating the current one.
func TestWorldHandlesExpire(t *testing.T) {
	h := &Host{}
	w := world.Config{Synchronous: true, Log: slog.New(slog.DiscardHandler)}.New()
	t.Cleanup(func() { _ = w.Close() })
	first, err := h.BindWorld("test", w)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := h.BindWorld("another", w); err == nil {
		t.Fatal("one world acquired two instance identities")
	}
	if err := h.UnbindWorld(t.Context(), first); err != nil {
		t.Fatal(err)
	}
	second, err := h.BindWorld(first.ID, w)
	if err != nil {
		t.Fatal(err)
	}
	if first.Generation == second.Generation {
		t.Fatal("reopened world reused its generation")
	}
	if _, err := h.ResolveWorld(first); !errors.Is(err, ErrStaleWorld) {
		t.Fatalf("stale resolve: %v", err)
	}
	if err := h.UnbindWorld(t.Context(), first); !errors.Is(err, ErrStaleWorld) {
		t.Fatalf("stale unbind: %v", err)
	}
	if resolved, err := h.ResolveWorld(second); err != nil || resolved != w {
		t.Fatalf("current resolve: %p, %v", resolved, err)
	}
}

// TestOldInstanceCallbacksFailAfterReopen rejects both queued work and already-read results.
func TestOldInstanceCallbacksFailAfterReopen(t *testing.T) {
	f := newIdleFixture(t, slog.New(slog.DiscardHandler), nil)
	pos := BlockPos{X: 1, Y: 100, Z: 40}
	f.place(probeCounter, pos, []byte("saved"))
	ev := event{w: f.w, instance: f.instance, anchor: pos.cube(), call: Call{
		ScheduledUpdate: &ScheduledUpdateCall{Pos: pos},
	}}
	var snap snapshot
	f.do(func(tx *world.Tx) { snap = f.host.read(tx, &dispatcher{id: "probe"}, ev) })
	if err := f.host.UnbindWorld(t.Context(), f.instance.handle); err != nil {
		t.Fatal(err)
	}
	if _, err := f.host.BindWorld(f.instance.handle.ID, f.w); err != nil {
		t.Fatal(err)
	}
	if _, err := f.host.snapshot(t.Context(), &dispatcher{id: "probe"}, &ev); !errors.Is(err, errStale) {
		t.Fatalf("queued callback from old opening: %v", err)
	}
	f.do(func(tx *world.Tx) {
		if _, err := f.host.apply(tx, "probe", ev, snap, nil); !errors.Is(err, errStale) {
			t.Fatalf("old callback result: %v", err)
		}
	})
	f.assertData("probe", pos, []byte("saved"))
}

// TestSameTypeTransferDropsFocus denies a source block's authority in another live world.
func TestSameTypeTransferDropsFocus(t *testing.T) {
	sup := &Supervisor{registered: Loaded{Focus: true}}
	f := newIdleClientFixture(t, slog.New(slog.DiscardHandler), map[string]*Supervisor{"probe": sup}, nil)
	pos := probePos(probeCount)
	f.place(probeCounter, pos, nil)
	f.moveTo(beside(pos))
	d := f.host.dispatchers["probe"]
	ev := event{w: f.w, instance: f.instance, actor: f.actor, anchor: pos.cube(), call: Call{
		Interact: &InteractCall{Player: f.actorID(), Pos: pos, Face: FaceUp},
	}}
	f.do(func(tx *world.Tx) { d.recordFocus(ev, f.host.read(tx, d, ev)) })
	other := world.Config{Synchronous: true, Log: slog.New(slog.DiscardHandler)}.New()
	t.Cleanup(func() { _ = other.Close() })
	handle, err := f.host.BindWorld("second", other)
	if err != nil {
		t.Fatal(err)
	}
	f.do(func(tx *world.Tx) { tx.RemoveEntity(f.player(tx)) })
	b, _ := f.host.reg.Lookup(probeCounter)
	if err := other.Do(func(tx *world.Tx) {
		tx.SetBlock(pos.cube(), b, nil)
		tx.AddEntity(f.actor)
	}).Wait(t.Context()); err != nil {
		t.Fatal(err)
	}
	message := event{actor: f.actor, call: Call{ClientMessage: &ClientMessageCall{Player: f.actorID()}}}
	snap, err := f.host.snapshot(t.Context(), d, &message)
	if err != nil {
		t.Fatal(err)
	}
	if message.w != other || message.instance.handle != handle || message.call.focus() != nil || len(snap.cells) != 0 {
		t.Fatalf("source focus survived transfer: instance %+v, focus %v, cells %d", message.instance, message.call.focus(), len(snap.cells))
	}
}

// TestStoreRejectsAmbiguousInstanceMigration fails before unknown identities can alias data.
func TestStoreRejectsAmbiguousInstanceMigration(t *testing.T) {
	for _, raw := range []string{
		`{"schema":1,"entries":[{"dim":99}]}`,
		`{"schema":1,"entries":[{"instance":"other","dim":0}]}`,
		`{"schema":1,"entries":[{}]}`,
		`{"schema":2,"entries":[{"instance":"other","dim":0}]}`,
		`{"schema":2,"entries":[{}]}`,
	} {
		dir := t.TempDir()
		if err := os.WriteFile(filepath.Join(dir, "probe.json"), []byte(raw), 0o600); err != nil {
			t.Fatal(err)
		}
		if _, err := OpenStore(dir); err == nil {
			t.Fatalf("accepted ambiguous store: %s", raw)
		}
	}
}
