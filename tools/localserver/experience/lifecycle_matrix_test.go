package experience

import (
	"context"
	"path/filepath"
	"testing"
	"time"

	ext "github.com/cinnabar-mc/cinnabar/tools/localserver/cinnabarext"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/player/playerdb"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
	"github.com/google/uuid"
)

// lifecycleChannels lets the real host consume a preparation made by an earlier matrix step.
type lifecycleChannels struct {
	*ext.Server
	prepared *ext.WorldChange
}

// PrepareWorldChange hands the existing reservation to the production transfer coordinator.
func (c *lifecycleChannels) PrepareWorldChange(id uuid.UUID) (*ext.WorldChange, error) {
	if c.prepared != nil {
		change := c.prepared
		c.prepared = nil
		return change, nil
	}
	return c.Server.PrepareWorldChange(id)
}

// lifecycleMatrix owns one player's synchronous worlds, persisted location and real client part.
type lifecycleMatrix struct {
	f        *runtimeFixture
	part     *lifecyclePart
	channels *lifecycleChannels
	provider player.Provider
	live     bool
	shutdown bool
	fenced   bool
	leaves   uint64
	tick     uint64
}

// newLifecycleMatrix uses a scripted helper to make each leave visibly persist a private write.
func newLifecycleMatrix(t *testing.T) *lifecycleMatrix {
	t.Helper()
	f := newRuntimeFixture(t)
	sup, _ := startFake(t, "leave_write", f.h.log, startOptions{})
	sup.registered.Worlds = f.d.sup.registered.Worlds
	f.d.sup = sup
	base, err := playerdb.NewProvider(filepath.Join(t.TempDir(), "players"))
	if err != nil {
		t.Fatal(err)
	}
	provider, err := f.h.PlayerProvider(base, t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = provider.Close() })
	part := newLifecyclePart(t, f)
	channels := &lifecycleChannels{Server: part.server}
	f.h.channels = channels
	return &lifecycleMatrix{f: f, part: part, channels: channels, provider: provider, live: true}
}

// depart queues exactly one leave while retaining the entity until a disconnect removes it.
func (m *lifecycleMatrix) depart(p *player.Player) {
	if m.f.h.epoch(p.H()) != 0 {
		m.leaves++
	}
	m.f.h.PlayerDisconnected(p)
}

// disconnect saves and removes the live session under its actual world transaction.
func (m *lifecycleMatrix) disconnect() {
	if !m.live {
		return
	}
	f := m.f
	if err := f.actor.Do(func(tx *world.Tx, e world.Entity) {
		p := e.(*player.Player)
		if err := m.provider.Save(p.UUID(), p.Data(), tx.World()); err != nil {
			f.t.Fatal(err)
		}
		m.depart(p)
		tx.RemoveEntity(p)
		_ = f.actor.Close()
	}).Wait(f.t.Context()); err != nil {
		f.t.Fatal(err)
	}
	_ = m.part.conn.Close()
	m.live = false
}

// step executes one lifecycle event; closed admission remains closed for all later steps.
func (m *lifecycleMatrix) step(name string) {
	f := m.f
	if m.shutdown && name != "disconnect" && name != "join" {
		return
	}
	switch name {
	case "join":
		if m.live && !m.shutdown && f.h.epoch(f.actor) == 0 {
			m.step("reconnect")
			return
		}
		if m.live {
			if err := f.actor.Do(func(_ *world.Tx, e world.Entity) { f.h.PlayerJoined(e.(*player.Player)) }).Wait(f.t.Context()); err != nil {
				f.t.Fatal(err)
			}
		}
	case "leave":
		if m.live {
			if err := f.actor.Do(func(_ *world.Tx, e world.Entity) { m.depart(e.(*player.Player)) }).Wait(f.t.Context()); err != nil {
				f.t.Fatal(err)
			}
		}
	case "disconnect":
		m.disconnect()
	case "reconnect":
		m.disconnect()
		id := f.actor.UUID()
		data, w, err := m.provider.Load(id, func(world.Dimension) *world.World { return f.source })
		if err != nil {
			f.t.Fatal(err)
		}
		f.actor = world.EntitySpawnOpts{ID: id}.New(player.Type, data)
		f.do(w, func(tx *world.Tx) { f.h.PlayerJoined(tx.AddEntity(f.actor).(*player.Player)) })
		m.live = true
		m.part = newLifecyclePart(f.t, f)
		// A reconnect establishes a new transport session; an old reservation is retired.
		m.channels.prepared.Abort()
		m.channels = &lifecycleChannels{Server: m.part.server}
		f.h.channels = m.channels
		m.fenced = false
	case "handshake":
		if m.live {
			m.part.handshake()
		}
	case "portal", "respawn":
		if !m.live {
			return
		}
		destinationID := ext.WorldID("probe:two")
		if name == "respawn" {
			destinationID = "probe:one"
		}
		handle, ok := f.h.worlds.handle(destinationID)
		if !ok {
			return
		}
		destination, _ := f.h.ResolveWorld(handle)
		var source *world.World
		if err := f.actor.Do(func(tx *world.Tx, e world.Entity) { source = tx.World(); tx.RemoveEntity(e) }).Wait(f.t.Context()); err != nil {
			f.t.Fatal(err)
		}
		if source != destination {
			f.do(destination, func(tx *world.Tx) { tx.AddEntityAt(f.actor, mgl64.Vec3{4, 75, 6}) })
		} else {
			f.do(source, func(tx *world.Tx) { tx.AddEntity(f.actor) })
		}
		m.tick++
		if err := f.h.PollPlayers(f.t.Context(), m.tick); err != nil {
			f.t.Fatal(err)
		}
	case "prepare":
		if !m.live {
			return
		}
		var err error
		m.channels.prepared, err = m.part.server.PrepareWorldChange(f.actor.UUID())
		if err != nil {
			f.t.Fatal(err)
		}
		m.fenced = true
	case "commit":
		if !m.live {
			return
		}
		if _, ok := f.h.worlds.handle("probe:one"); !ok {
			return
		}
		// Failure after leave is expected: only admitted sessions can transfer.
		err := f.travel(f.t.Context(), "probe:one")
		if err == nil {
			m.fenced = true
		}
	case "unload":
		handle, ok := f.h.worlds.handle("probe:one")
		if ok {
			_ = f.h.unloadWorld(f.t.Context(), "probe", handle)
		}
	case "shutdown":
		m.drain()
	default:
		f.t.Fatalf("unknown lifecycle event %s", name)
	}
}

// drain finishes every admitted callback and verifies persisted final writes before worlds close.
func (m *lifecycleMatrix) drain() {
	if m.shutdown {
		return
	}
	f := m.f
	if f.h.epoch(f.actor) != 0 {
		m.leaves++
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := f.h.Drain(ctx); err != nil {
		f.t.Fatal(err)
	}
	m.shutdown = true
	m.channels.prepared.Abort()
	reopened := openTestStore(f.t, f.h.store.dir)
	state := reopened.readStates("probe", []stateAddress{{Scope: StorePlayer, Owner: f.actor.UUID().String()}})[0]
	if state.Revision != m.leaves || m.leaves != 0 && (state.Data == nil || *state.Data != "01") {
		f.t.Fatalf("final writes = %+v, want %d leaves", state, m.leaves)
	}
}

// check asserts physical membership, current-session authority and prepared transport fencing.
func (m *lifecycleMatrix) check() {
	f := m.f
	want := 0
	if m.live {
		want = 1
	}
	if got := f.countLocations(); got != want {
		f.t.Fatalf("authoritative memberships = %d, want %d", got, want)
	}
	if m.shutdown && f.h.epoch(f.actor) != 0 {
		f.t.Fatal("shutdown admitted a new session")
	}
	if m.fenced && m.part.ready && m.live {
		before := m.part.admitted
		m.part.oldInput()
		if m.part.admitted != before {
			f.t.Fatal("unfenced old-epoch work")
		}
	}
}

// TestLifecycleEventPairs runs every pair in both orders through actual host and extension code.
func TestLifecycleEventPairs(t *testing.T) {
	events := []string{"join", "leave", "disconnect", "reconnect", "handshake", "portal", "respawn", "prepare", "commit", "unload", "shutdown"}
	for i, first := range events {
		for _, second := range events[i+1:] {
			for _, pair := range [][2]string{{first, second}, {second, first}} {
				t.Run(pair[0]+"/"+pair[1], func(t *testing.T) {
					m := newLifecycleMatrix(t)
					old, _ := m.f.request()
					m.step(pair[0])
					m.check()
					m.step(pair[1])
					m.check()
					if m.f.actor != old.actor || m.f.h.epoch(old.actor) != old.epoch {
						if m.f.h.playerCurrent(old) {
							t.Fatal("old session or world epoch retained authority")
						}
					}
					m.drain()
					m.check()
				})
			}
		}
	}
}
