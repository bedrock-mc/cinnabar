package experience

import (
	"bytes"
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"
	"sync"
	"testing"

	"github.com/cinnabar-mc/cinnabar/tools/localserver/cinnabarext"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/google/uuid"
)

// observedCancellation lets a transaction finish only after its waiter observes cancellation.
type observedCancellation struct {
	context.Context
	observed chan struct{}
	once     sync.Once
}

// Err reports cancellation and releases the scripted transaction once the wait has returned.
func (c *observedCancellation) Err() error {
	err := c.Context.Err()
	if err != nil {
		c.once.Do(func() { close(c.observed) })
	}
	return err
}

// recoveryInstance opens an isolated world without disturbing the matrix player's destination.
func (m *lifecycleMatrix) recoveryInstance(name string, persistent bool) (WorldInstance, WorldHandle) {
	f := m.f
	def := WorldDefinition{ID: WorldDefinitionID("probe:" + name), Settings: WorldSettings{
		Generator: GeneratorEmpty, Dimension: DimensionOverworld, Persistent: persistent,
	}}
	f.h.manager.definitions[def.ID] = def
	instance := WorldInstance{ID: cinnabarext.WorldID("probe:" + name), Definition: def.ID}
	handle, err := f.h.createWorld(f.t.Context(), "probe", instance)
	if err != nil {
		f.t.Fatal(err)
	}
	return instance, handle
}

// cancelUnload cancels a running unbind and verifies that reopening can acquire the database.
func (m *lifecycleMatrix) cancelUnload() {
	f := m.f
	f.h.manager.synchronous = false
	instance, handle := m.recoveryInstance("cancelled", true)
	f.h.manager.synchronous = true
	owned := f.h.manager.owned[handle]
	base, cancel := context.WithCancel(f.t.Context())
	defer cancel()
	ctx := &observedCancellation{Context: base, observed: make(chan struct{})}
	f.h.manager.afterUnbind = func() {
		cancel()
		<-ctx.observed
	}
	defer func() { f.h.manager.afterUnbind = nil }()
	if err := f.h.unloadWorld(ctx, "probe", handle); !errors.Is(err, context.Canceled) {
		f.t.Fatalf("unload cancellation = %v", err)
	}
	if _, ok := f.h.manager.owned[handle]; ok {
		f.t.Fatal("cancelled unload retained ownership")
	}
	if err := owned.w.Do(func(*world.Tx) {}).Wait(f.t.Context()); !errors.Is(err, world.ErrWorldClosed) {
		f.t.Fatalf("unbound world remained open: %v", err)
	}
	fresh, err := f.h.createWorld(f.t.Context(), "probe", instance)
	if err != nil {
		f.t.Fatalf("reopen after cancelled unload: %v", err)
	}
	if err := f.h.releaseWorld(handle, owned); err != nil {
		f.t.Fatalf("repeated release: %v", err)
	}
	if _, err := f.h.ResolveWorld(fresh); err != nil {
		f.t.Fatalf("repeated release invalidated the new opening: %v", err)
	}
}

// interruptMetadata injects a partial write and opens the instance using the last complete record.
func (m *lifecycleMatrix) interruptMetadata() {
	f := m.f
	instance, handle := m.recoveryInstance("metadata", true)
	if err := f.h.unloadWorld(f.t.Context(), "probe", handle); err != nil {
		f.t.Fatal(err)
	}
	path := filepath.Join(f.h.manager.root, "probe", "metadata", "experience-world.json")
	raw, err := os.ReadFile(path)
	if err != nil {
		f.t.Fatal(err)
	}
	// Equivalent JSON with different whitespace detects an unnecessary metadata rewrite.
	raw = append(raw, '\n')
	if err := os.WriteFile(path, raw, 0o600); err != nil {
		f.t.Fatal(err)
	}
	interrupted := errors.New("interrupted metadata writer")
	if err := writeAtomicFile(path, func(w io.Writer) error {
		if _, err := io.WriteString(w, "{\"id\":"); err != nil {
			return err
		}
		return interrupted
	}); !errors.Is(err, interrupted) {
		f.t.Fatalf("interrupted write = %v", err)
	}
	if _, err := f.h.createWorld(f.t.Context(), "probe", instance); err != nil {
		f.t.Fatalf("reopen after interrupted metadata write: %v", err)
	}
	if got, err := os.ReadFile(path); err != nil || !bytes.Equal(got, raw) {
		f.t.Fatalf("validated metadata changed: %q, %v", got, err)
	}
}

// reopenAfterCrash reloads the last flushed records without persisting graceful terrain cleanup.
func (m *lifecycleMatrix) reopenAfterCrash() {
	f := m.f
	instance, temporary := m.recoveryInstance("crashed", false)
	_, saved := m.recoveryInstance("survivor", true)
	addresses := stateAddresses(instance.ID, nil)
	actor := uuid.Nil.String()
	addresses = append(addresses, stateAddress{Scope: StorePlayer, Owner: actor})
	for _, exp := range []string{"recovery", "other_recovery"} {
		for _, handle := range []WorldHandle{temporary, saved} {
			key := Key{Instance: handle.ID, Y: 70}
			f.h.store.Place(exp, key)
			if err := f.h.store.SetData(exp, key, []byte("block"), true); err != nil {
				f.t.Fatal(err)
			}
		}
		for _, address := range addresses {
			if err := f.h.store.setState(exp, address, StoreKey{Scope: address.Scope, Schema: 1}, []byte("state"), true); err != nil {
				f.t.Fatal(err)
			}
		}
	}
	if err := f.h.store.Flush(); err != nil {
		f.t.Fatal(err)
	}
	// Release operating-system resources, then abandon the unflushed cleanup just as a crash does.
	if err := f.h.unloadWorld(f.t.Context(), "probe", temporary); err != nil {
		f.t.Fatal(err)
	}
	f.h.store = openTestStore(f.t, f.h.store.dir)
	if _, err := f.h.createWorld(f.t.Context(), "probe", instance); err != nil {
		f.t.Fatal(err)
	}
	if err := f.h.store.Flush(); err != nil {
		f.t.Fatal(err)
	}
	for _, store := range []*Store{f.h.store, openTestStore(f.t, f.h.store.dir)} {
		for _, exp := range []string{"recovery", "other_recovery"} {
			if _, ok := store.Token(exp, Key{Instance: temporary.ID, Y: 70}); ok {
				f.t.Fatal("fresh temporary terrain retained an orphaned placement")
			}
			if got, ok := store.Data(exp, Key{Instance: saved.ID, Y: 70}); !ok || string(got) != "block" {
				f.t.Fatal("persistent placement lost during reconciliation")
			}
			for _, record := range store.readStates(exp, addresses) {
				if record.Data == nil || *record.Data != "7374617465" || record.Revision != 1 || record.Key.Schema != 1 {
					f.t.Fatalf("scoped record changed: %+v", record)
				}
			}
			if store.Budget(exp) != dataQuota-20 {
				f.t.Fatalf("recovered quota = %d, want %d", store.Budget(exp), dataQuota-20)
			}
		}
	}
}

// TestLifecycleRecoveryScenarios checks cancellation and crash invariants before any shutdown drain.
func TestLifecycleRecoveryScenarios(t *testing.T) {
	for _, name := range []string{"cancel_unload", "metadata_interrupt", "crash_reopen"} {
		t.Run(name, func(t *testing.T) {
			probe := &lifecycleProbe{opened: make(map[*world.World]bool)}
			m := newLifecycleMatrix(t, probe)
			m.step(name)
			for w := range probe.opened {
				if _, bound := m.f.h.worlds.of(w); !bound {
					t.Fatal("unbound world retained lifecycle registration")
				}
			}
			m.check()
		})
	}
}
