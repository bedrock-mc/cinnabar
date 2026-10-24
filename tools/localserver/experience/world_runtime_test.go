package experience

import (
	"context"
	"errors"
	"log/slog"
	"testing"

	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/entity"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
	"github.com/google/uuid"
)

// runtimeFixture owns synchronous worlds and a session, without a live network or worker.
type runtimeFixture struct {
	t      *testing.T
	h      *Host
	source *world.World
	actor  *world.EntityHandle
	d      *dispatcher
}

// newRuntimeFixture opens one default world and two guest-owned destination definitions.
func newRuntimeFixture(t *testing.T) *runtimeFixture {
	t.Helper()
	registration := &WorldRegistration{Definitions: []WorldDefinition{
		{ID: "probe:plain", Settings: WorldSettings{Generator: GeneratorEmpty, Dimension: DimensionOverworld}},
		{ID: "probe:hot", Settings: WorldSettings{Generator: GeneratorFlat, Dimension: DimensionNether}},
	}, Startup: []WorldInstance{{ID: "probe:one", Definition: "probe:plain"}, {ID: "probe:two", Definition: "probe:hot"}}}
	log := slog.New(slog.DiscardHandler)
	h := NewHost(&Registry{}, openTestStore(t, t.TempDir()), map[string]*Supervisor{"probe": {registered: Loaded{Worlds: registration}}}, "world", nil, log)
	h.manager.synchronous = true
	w := world.Config{Synchronous: true, Log: log, Entities: entity.DefaultRegistry}.New()
	if _, err := h.BindWorld(OverworldInstance, w); err != nil {
		t.Fatal(err)
	}
	if err := h.ConfigureWorlds(t.Context(), t.TempDir()); err != nil {
		t.Fatal(err)
	}
	f := &runtimeFixture{t: t, h: h, source: w, d: h.dispatchers["probe"]}
	f.actor = world.EntitySpawnOpts{Position: mgl64.Vec3{0, 70, 0}}.New(player.Type, player.Config{Name: "Actor"})
	f.do(w, func(tx *world.Tx) { h.PlayerJoined(tx.AddEntity(f.actor).(*player.Player)) })
	t.Cleanup(func() {
		if err := h.Close(); err != nil {
			t.Error(err)
		}
		_ = w.Close()
	})
	return f
}

// do finishes one synchronous transaction before the fixture advances.
func (f *runtimeFixture) do(w *world.World, fn func(*world.Tx)) {
	f.t.Helper()
	if err := w.Do(fn).Wait(f.t.Context()); err != nil {
		f.t.Fatal(err)
	}
}

// request snapshots a callback under the session's current epoch.
func (f *runtimeFixture) request() (event, snapshot) {
	f.t.Helper()
	ev := event{actor: f.actor, epoch: f.h.epoch(f.actor), call: Call{ClientMessage: &ClientMessageCall{Player: f.actor.UUID().String(), Payload: []Scalar{}}}}
	snap, err := f.h.snapshot(f.t.Context(), f.d, &ev)
	if err != nil {
		f.t.Fatal(err)
	}
	return ev, snap
}

// travel runs the same exclusive lifecycle result the helper emits.
func (f *runtimeFixture) travel(ctx context.Context, id WorldInstanceID) error {
	f.t.Helper()
	ev, snap := f.request()
	handle, ok := f.h.worlds.handle(id)
	if !ok {
		f.t.Fatal("destination missing")
	}
	return f.h.commitLifecycle(ctx, f.d, ev, snap, []Op{{World: &WorldActionOp{Action: WorldOp{Type: "transfer", Player: f.actor.UUID().String(), Instance: handle, Position: Position{Y: 70}}}}})
}

// countLocations counts actual Dragonfly membership, rather than trusting coordinator metadata.
func (f *runtimeFixture) countLocations() int {
	f.t.Helper()
	count := 0
	worlds := []*world.World{f.source}
	for _, w := range f.h.manager.owned {
		worlds = append(worlds, w)
	}
	for _, w := range worlds {
		f.do(w, func(tx *world.Tx) {
			for p := range tx.Players() {
				if p.H() == f.actor {
					count++
				}
			}
		})
	}
	return count
}

// TestWorldRuntimeLifecycle exercises startup, runtime creation, occupied unload and stale handles.
func TestWorldRuntimeLifecycle(t *testing.T) {
	f := newRuntimeFixture(t)
	if err := f.travel(t.Context(), "probe:one"); err != nil {
		t.Fatal(err)
	}
	one, _ := f.h.worlds.handle("probe:one")
	if err := f.h.unloadWorld(t.Context(), "probe", one); err == nil {
		t.Fatal("unloaded occupied world")
	}
	if f.countLocations() != 1 {
		t.Fatal("transfer did not leave one membership")
	}
	if err := f.travel(t.Context(), "probe:two"); err != nil {
		t.Fatal(err)
	}
	if err := f.h.unloadWorld(t.Context(), "probe", one); err != nil {
		t.Fatal(err)
	}
	fresh, err := f.h.createWorld(t.Context(), "probe", WorldInstance{ID: "probe:one", Definition: "probe:plain"})
	if err != nil {
		t.Fatal(err)
	}
	if fresh == one {
		t.Fatal("opening reused generation")
	}
	if _, err := f.h.ResolveWorld(one); !errors.Is(err, ErrStaleWorld) {
		t.Fatalf("old handle resolved: %v", err)
	}
	if _, err := f.h.createWorld(t.Context(), "other", WorldInstance{ID: "other:one", Definition: "probe:plain"}); err == nil {
		t.Fatal("borrowed another mod's definition")
	}
	if f.countLocations() != 1 {
		t.Fatal("cross-type transfer duplicated the player")
	}
}

// TestDepartureReturnFencesCallbacks covers an old result in the same live world after a round trip.
func TestDepartureReturnFencesCallbacks(t *testing.T) {
	f := newRuntimeFixture(t)
	if err := f.travel(t.Context(), "probe:one"); err != nil {
		t.Fatal(err)
	}
	old, snap := f.request()
	if err := f.travel(t.Context(), "probe:two"); err != nil {
		t.Fatal(err)
	}
	if err := f.travel(t.Context(), "probe:one"); err != nil {
		t.Fatal(err)
	}
	if _, err := f.h.snapshot(t.Context(), f.d, &old); !errors.Is(err, errStale) {
		t.Fatalf("old queued callback survived: %v", err)
	}
	f.do(old.w, func(tx *world.Tx) {
		if _, err := f.h.apply(tx, "probe", old, snap, nil); !errors.Is(err, errStale) {
			t.Fatalf("old result survived return: %v", err)
		}
	})
	if f.countLocations() != 1 {
		t.Fatal("round trip duplicated player")
	}
}

// TestTransferFailureRestoresOneLocation covers pre-detach cancellation and failed admission.
func TestTransferFailureRestoresOneLocation(t *testing.T) {
	for _, mode := range []string{"cancel_before", "admission", "cancel_detached"} {
		t.Run(mode, func(t *testing.T) {
			f := newRuntimeFixture(t)
			before := f.h.epoch(f.actor)
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			switch mode {
			case "cancel_before":
				cancel()
			case "admission":
				f.h.manager.admit = func() error { return errors.New("admission failed") }
			case "cancel_detached":
				f.h.manager.admit = func() error { cancel(); return ctx.Err() }
			}
			if err := f.travel(ctx, "probe:one"); err == nil {
				t.Fatal("failure reported success")
			}
			if f.countLocations() != 1 {
				t.Fatal("failure stranded or duplicated player")
			}
			f.h.players.mu.Lock()
			state := *f.h.players.items[f.actor]
			f.h.players.mu.Unlock()
			if state.w != f.source || state.transferring {
				t.Fatal("source recovery did not finish")
			}
			if mode != "cancel_before" && state.location.Epoch <= before {
				t.Fatal("recovery revived old callback authority")
			}
		})
	}
}

// TestDetachedDisconnectClosesAuthority does not restore a handle that disconnected while detached.
func TestDetachedDisconnectClosesAuthority(t *testing.T) {
	f := newRuntimeFixture(t)
	f.h.manager.admit = func() error { _ = f.actor.Close(); return world.ErrEntityClosed }
	if err := f.travel(t.Context(), "probe:one"); !errors.Is(err, world.ErrEntityClosed) {
		t.Fatalf("disconnect: %v", err)
	}
	if f.countLocations() != 0 || f.h.epoch(f.actor) != 0 {
		t.Fatal("disconnected player retained authority")
	}
}

// TestPlayerObservationsAreOrderedAndCoalesced delivers immutable history after departure.
func TestPlayerObservationsAreOrderedAndCoalesced(t *testing.T) {
	f := newRuntimeFixture(t)
	f.do(f.source, func(tx *world.Tx) {
		p, _ := f.actor.Entity(tx)
		p.(*player.Player).Teleport(mgl64.Vec3{1, 70, 0})
		p.(*player.Player).Teleport(mgl64.Vec3{2, 70, 0})
	})
	if err := f.h.PollPlayers(t.Context(), 1); err != nil {
		t.Fatal(err)
	}
	if err := f.h.PollPlayers(t.Context(), 1); err != nil {
		t.Fatal(err)
	}
	if err := f.travel(t.Context(), "probe:one"); err != nil {
		t.Fatal(err)
	}
	one, _ := f.h.worlds.handle("probe:one")
	w, _ := f.h.ResolveWorld(one)
	f.do(w, func(tx *world.Tx) {
		p, _ := f.actor.Entity(tx)
		f.h.PlayerDisconnected(p.(*player.Player))
		tx.RemoveEntity(p)
		_ = f.actor.Close()
	})
	kinds := []string{"join", "move", "world_change", "leave"}
	for _, kind := range kinds {
		select {
		case ev := <-f.d.events:
			if ev.call.Player.Event.Type != kind {
				t.Fatalf("got %s, want %s", ev.call.Player.Event.Type, kind)
			}
			snap, err := f.h.snapshot(t.Context(), f.d, &ev)
			if err != nil {
				t.Fatal(err)
			}
			if kind == "move" && (ev.call.Player.Event.Before.Position.X != 0 || ev.call.Player.Event.After.Position.X != 2) {
				t.Fatal("move did not coalesce committed endpoints")
			}
			if snap.req.Call.Player == nil {
				t.Fatal("lifecycle event lost after player left")
			}
		default:
			t.Fatalf("missing %s", kind)
		}
	}
	if len(f.d.events) != 0 {
		t.Fatal("extra movement in the same tick")
	}
}

// TestClientFenceFailureKeepsOneDestination blocks new callbacks after a failed world notice.
func TestClientFenceFailureKeepsOneDestination(t *testing.T) {
	f := newRuntimeFixture(t)
	f.h.channels = runtimeChannels{}
	f.h.SetWorldChangeNotifier(func(uuid.UUID, WorldHandle, uint64) bool { return false })
	if err := f.travel(t.Context(), "probe:one"); err == nil {
		t.Fatal("failed notice reported success")
	}
	if f.countLocations() != 1 {
		t.Fatal("failed notice lost authoritative destination")
	}
	ev := event{actor: f.actor, epoch: f.h.epoch(f.actor)}
	if f.h.playerCurrent(ev) {
		t.Fatal("failed notice released callback fence")
	}
}

// TestWorldGenerationsSurviveRestart rejects handles saved by an earlier host process.
func TestWorldGenerationsSurviveRestart(t *testing.T) {
	dir := t.TempDir()
	log := slog.New(slog.DiscardHandler)
	w := world.Config{Synchronous: true, Log: log}.New()
	defer w.Close()
	h := &Host{store: openTestStore(t, dir)}
	old, err := h.BindWorld("probe:saved", w)
	if err != nil {
		t.Fatal(err)
	}
	restarted := &Host{store: openTestStore(t, dir)}
	current, err := restarted.BindWorld("probe:saved", w)
	if err != nil {
		t.Fatal(err)
	}
	if old == current {
		t.Fatal("restart revived old handle")
	}
	if _, err := restarted.ResolveWorld(old); !errors.Is(err, ErrStaleWorld) {
		t.Fatalf("previous process handle resolved: %v", err)
	}
}

// runtimeChannels supplies the enabled-client-parts condition without sending network data.
type runtimeChannels struct{ sent *int }

// Declares refuses undeclared test channels.
func (runtimeChannels) Declares(string, string, uint16) bool { return true }

// Send keeps fixture data off the network.
func (c runtimeChannels) Send(uuid.UUID, string, string, uint16, []Scalar) bool {
	if c.sent != nil {
		*c.sent++
		return true
	}
	return false
}

// TestDeferredSendsAndDetachedInputRespectEpochs covers both sides of the callback queue fence.
func TestDeferredSendsAndDetachedInputRespectEpochs(t *testing.T) {
	f := newRuntimeFixture(t)
	if err := f.travel(t.Context(), "probe:one"); err != nil {
		t.Fatal(err)
	}
	old, _ := f.request()
	f.h.manager.admit = func() error {
		if f.h.DeliverClientMessage(f.actor, "probe", "probe.test", 1, nil) {
			t.Fatal("old-world input admitted while detached")
		}
		return nil
	}
	if err := f.travel(t.Context(), "probe:two"); err != nil {
		t.Fatal(err)
	}
	if err := f.travel(t.Context(), "probe:one"); err != nil {
		t.Fatal(err)
	}
	sent := 0
	f.h.channels = runtimeChannels{sent: &sent}
	op := &SendClientOp{Player: f.actor.UUID().String(), Channel: "probe.test", Schema: 1}
	f.h.sendCurrent(f.d, old, op)
	if sent != 0 {
		t.Fatal("old-world deferred send escaped after return")
	}
	current, _ := f.request()
	f.h.sendCurrent(f.d, current, op)
	if sent != 1 {
		t.Fatal("current epoch could not send")
	}
}

// TestPersistentInstanceReopensTerrainAndState preserves storage while invalidating opening handles.
func TestPersistentInstanceReopensTerrainAndState(t *testing.T) {
	log := slog.New(slog.DiscardHandler)
	registration := &WorldRegistration{Definitions: []WorldDefinition{{ID: "probe:saved", Settings: WorldSettings{Generator: GeneratorEmpty, Dimension: DimensionOverworld, Persistent: true}}}}
	h := NewHost(&Registry{}, openTestStore(t, t.TempDir()), map[string]*Supervisor{"probe": {registered: Loaded{Worlds: registration}}}, "world", nil, log)
	h.manager.synchronous = true
	t.Cleanup(func() {
		if err := h.Close(); err != nil {
			t.Error(err)
		}
	})
	if err := h.ConfigureWorlds(t.Context(), t.TempDir()); err != nil {
		t.Fatal(err)
	}
	instance := WorldInstance{ID: "probe:saved", Definition: "probe:saved"}
	old, err := h.createWorld(t.Context(), "probe", instance)
	if err != nil {
		t.Fatal(err)
	}
	w, _ := h.ResolveWorld(old)
	pos := cube.Pos{0, 70, 0}
	if err := w.Do(func(tx *world.Tx) { tx.SetBlock(pos, block.Stone{}, nil) }).Wait(t.Context()); err != nil {
		t.Fatal(err)
	}
	address := stateAddress{Scope: StoreInstance, Owner: string(instance.ID)}
	if err := h.store.setState("probe", address, StoreKey{Scope: StoreInstance, Schema: 2}, []byte("saved"), true); err != nil {
		t.Fatal(err)
	}
	if err := h.unloadWorld(t.Context(), "probe", old); err != nil {
		t.Fatal(err)
	}
	fresh, err := h.createWorld(t.Context(), "probe", instance)
	if err != nil {
		t.Fatal(err)
	}
	if fresh == old {
		t.Fatal("reused persistent opening")
	}
	w, _ = h.ResolveWorld(fresh)
	if err := w.Do(func(tx *world.Tx) {
		if tx.Block(pos) != (block.Stone{}) {
			t.Fatal("persistent terrain lost")
		}
	}).Wait(t.Context()); err != nil {
		t.Fatal(err)
	}
	state := h.store.readStates("probe", []stateAddress{address})[0]
	if state.Data == nil || *state.Data != "7361766564" {
		t.Fatal("instance reopening lost its private state")
	}
}

// TestSynchronousEpochAcknowledgmentFollowsWorldEvent supports reentrant extension bridges.
func TestSynchronousEpochAcknowledgmentFollowsWorldEvent(t *testing.T) {
	f := newRuntimeFixture(t)
	f.h.channels = runtimeChannels{}
	f.h.SetWorldChangeNotifier(func(id uuid.UUID, handle WorldHandle, epoch uint64) bool {
		if f.h.epoch(f.actor) != epoch || id != f.actor.UUID() || handle.ID != "probe:one" {
			t.Fatal("notice preceded server epoch publication")
		}
		return f.h.DeliverEpoch(f.actor, "probe")
	})
	if err := f.travel(t.Context(), "probe:one"); err != nil {
		t.Fatal(err)
	}
	for _, kind := range []string{"join", "world_change", "epoch"} {
		select {
		case ev := <-f.d.events:
			if kind == "epoch" {
				if ev.call.Epoch == nil || ev.epoch != f.h.epoch(f.actor) {
					t.Fatal("acknowledgment lost new epoch")
				}
			} else if ev.call.Player == nil || ev.call.Player.Event.Type != kind {
				t.Fatalf("missing ordered %s", kind)
			}
		default:
			t.Fatalf("missing %s", kind)
		}
	}
}
