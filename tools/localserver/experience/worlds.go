package experience

import (
	"context"
	"errors"
	"fmt"
	"math"
	"strings"
	"sync"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/world"
)

// WorldInstanceID is a stable storage identity supplied by the server when it opens a world.
// Reopening that instance keeps its ID; another instance must have another ID.
type WorldInstanceID string

// WorldHandle identifies one live opening of an instance. Closing it invalidates the handle.
type WorldHandle struct {
	ID         WorldInstanceID
	Generation uint64
}

// The default instance IDs also identify the worlds in a migrated dimension-keyed store.
const (
	OverworldInstance WorldInstanceID = "overworld"
	NetherInstance    WorldInstanceID = "nether"
	EndInstance       WorldInstanceID = "end"
)

// ErrStaleWorld rejects a handle after its instance was unbound or reopened.
var ErrStaleWorld = errors.New("stale world handle")

// worldInstance is a live world binding. Its dimension name is legacy callback metadata,
// never its storage identity.
type worldInstance struct {
	handle WorldHandle
	id     string
}

// storeKey identifies pos within this instance independently of its dimension type.
func (w worldInstance) storeKey(pos cube.Pos) Key {
	return Key{Instance: w.handle.ID, X: int32(pos[0]), Y: int32(pos[1]), Z: int32(pos[2])}
}

// worldBindings tracks loaded instances and rejects handles from earlier openings.
// It does not own Dragonfly world shutdown.
type worldBindings struct {
	mu      sync.RWMutex
	next    uint64
	byWorld map[*world.World]worldInstance
	byID    map[WorldInstanceID]*world.World
}

// bind associates a stable instance ID with a live world and assigns a fresh generation.
func (b *worldBindings) bind(id WorldInstanceID, w *world.World) (WorldHandle, error) {
	if id == "" || strings.TrimSpace(string(id)) != string(id) || w == nil {
		return WorldHandle{}, errors.New("a nonempty instance ID and world are required")
	}
	dim, ok := world.DimensionID(w.Dimension())
	if !ok {
		return WorldHandle{}, errors.New("world dimension type is not registered")
	}
	name, ok := world.DimensionName(dim)
	if !ok {
		return WorldHandle{}, errors.New("world dimension type has no name")
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	if _, exists := b.byID[id]; exists {
		return WorldHandle{}, fmt.Errorf("world instance %q is already bound", id)
	}
	if _, exists := b.byWorld[w]; exists {
		return WorldHandle{}, errors.New("world is already bound to an instance")
	}
	if b.next == math.MaxUint64 {
		return WorldHandle{}, errors.New("world handle generations exhausted")
	}
	if b.byWorld == nil {
		b.byWorld = make(map[*world.World]worldInstance)
		b.byID = make(map[WorldInstanceID]*world.World)
	}
	b.next++
	handle := WorldHandle{ID: id, Generation: b.next}
	b.byWorld[w] = worldInstance{handle: handle, id: strings.ToLower(name)}
	b.byID[id] = w
	return handle, nil
}

// of returns a world's current instance binding, without inferring it from its type.
func (b *worldBindings) of(w *world.World) (worldInstance, bool) {
	b.mu.RLock()
	defer b.mu.RUnlock()
	i, ok := b.byWorld[w]
	return i, ok
}

// resolve returns the world only while handle names its current opening.
func (b *worldBindings) resolve(handle WorldHandle) (*world.World, error) {
	b.mu.RLock()
	defer b.mu.RUnlock()
	w := b.byID[handle.ID]
	if w == nil || b.byWorld[w].handle != handle {
		return nil, ErrStaleWorld
	}
	return w, nil
}

// unbind invalidates handle before the caller closes its world.
func (b *worldBindings) unbind(handle WorldHandle) error {
	b.mu.Lock()
	defer b.mu.Unlock()
	w := b.byID[handle.ID]
	if w == nil || b.byWorld[w].handle != handle {
		return ErrStaleWorld
	}
	delete(b.byID, handle.ID)
	delete(b.byWorld, w)
	return nil
}

// legacyWorldInstance maps only the historical store's three dimension identities.
func legacyWorldInstance(dim int) (WorldInstanceID, error) {
	switch dim {
	case 0:
		return OverworldInstance, nil
	case 1:
		return NetherInstance, nil
	case 2:
		return EndInstance, nil
	default:
		return "", fmt.Errorf("legacy store has unknown dimension %d", dim)
	}
}

// BindWorld enables Experience hooks for a live instance. The caller owns its lifetime and
// must unbind it before closing it or replacing it with another world.
func (h *Host) BindWorld(id WorldInstanceID, w *world.World) (WorldHandle, error) {
	if h.closed.Load() {
		return WorldHandle{}, errors.New("experience host is closed")
	}
	return h.worlds.bind(id, w)
}

// ResolveWorld rejects stale handles instead of resolving them to a later instance opening.
func (h *Host) ResolveWorld(handle WorldHandle) (*world.World, error) {
	if h.closed.Load() {
		return nil, ErrStaleWorld
	}
	return h.worlds.resolve(handle)
}

// UnbindWorld invalidates queued block callbacks and already-read results after earlier
// world transactions finish. Call it outside a transaction, before closing the world.
func (h *Host) UnbindWorld(ctx context.Context, handle WorldHandle) error {
	w, err := h.ResolveWorld(handle)
	if err != nil {
		return err
	}
	var result error
	if err := await(ctx, w.Do(func(*world.Tx) { result = h.worlds.unbind(handle) })); err != nil {
		return err
	}
	return result
}

// BindDefaultWorlds binds the server's existing three worlds to their migration identities.
// Additional instances use BindWorld with their own stable IDs.
func (h *Host) BindDefaultWorlds(worlds ...*world.World) error {
	for _, w := range worlds {
		dim, ok := world.DimensionID(w.Dimension())
		if !ok {
			return errors.New("default world has an unregistered dimension type")
		}
		id, err := legacyWorldInstance(dim)
		if err != nil {
			return err
		}
		if _, err := h.BindWorld(id, w); err != nil {
			return err
		}
	}
	return nil
}
