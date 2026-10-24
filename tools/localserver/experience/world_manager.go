package experience

import (
	"context"
	"errors"
	"fmt"
	"slices"
	"strings"
	"sync"

	"github.com/cinnabar-mc/cinnabar/tools/localserver/cinnabarext"

	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/entity"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/df-mc/dragonfly/server/world/biome"
	"github.com/df-mc/dragonfly/server/world/generator"
)

// worldManager serializes lifecycle changes outside world transactions. Definitions freeze at startup.
type worldManager struct {
	mu          sync.Mutex
	root        string
	synchronous bool
	definitions map[WorldDefinitionID]WorldDefinition
	owned       map[WorldHandle]ownedWorld
	lifecycle   WorldLifecycle
	// admit is a test seam for a failure after detach but before destination admission.
	admit func() error
}

// ownedWorld keeps the terrain and its storage policy together for every close path.
type ownedWorld struct {
	w          *world.World
	persistent bool
}

// WorldLifecycle applies local world settings and tracks simulation ownership.
// Hooks run outside world transactions, before admission and before close.
type WorldLifecycle interface {
	// WorldOpened initializes a world before players or callbacks can use it.
	WorldOpened(*world.World)
	// WorldClosed releases the world from local simulation ownership.
	WorldClosed(*world.World)
}

// ownedWorldName permits stable mod-scoped IDs without accepting filesystem path components.
func ownedWorldName(id, owner string) bool {
	name, ok := strings.CutPrefix(id, owner+":")
	if !ok || len(name) == 0 || len(name) > maxNameBytes {
		return false
	}
	for _, c := range name {
		if !(c >= 'a' && c <= 'z' || c >= '0' && c <= '9' || c == '_') {
			return false
		}
	}
	return true
}

// ConfigureWorlds validates declarations and opens startup instances before admission.
// Callers may supply one lifecycle owner to initialize and track every instance.
func (h *Host) ConfigureWorlds(ctx context.Context, root string, lifecycle ...WorldLifecycle) error {
	if len(lifecycle) > 1 {
		return errors.New("only one world lifecycle owner may be configured")
	}
	h.manager.mu.Lock()
	defer h.manager.mu.Unlock()
	if h.manager.definitions != nil {
		return errors.New("world definitions already configured")
	}
	definitions := make(map[WorldDefinitionID]WorldDefinition)
	var startup []WorldInstance
	seen := map[cinnabarext.WorldID]bool{}
	for id, d := range h.dispatchers {
		if d.sup.registered.Worlds == nil {
			continue
		}
		r := d.sup.registered.Worlds
		for _, def := range r.Definitions {
			if !ownedWorldName(string(def.ID), id) {
				return errors.New("world definition outside mod namespace")
			}
			if _, exists := definitions[def.ID]; exists {
				return errors.New("duplicate world definition")
			}
			if _, err := definitionDimension(def.Settings); err != nil {
				return err
			}
			definitions[def.ID] = def
		}
		for _, instance := range r.Startup {
			if !ownedWorldName(string(instance.ID), id) || !ownedWorldName(string(instance.Definition), id) || seen[instance.ID] {
				return errors.New("invalid startup instance")
			}
			seen[instance.ID] = true
			startup = append(startup, instance)
		}
	}
	for _, instance := range startup {
		if _, ok := definitions[instance.Definition]; !ok {
			return errors.New("startup definition unavailable")
		}
	}
	h.manager.root, h.manager.definitions, h.manager.owned = root, definitions, make(map[WorldHandle]ownedWorld)
	if len(lifecycle) == 1 {
		h.manager.lifecycle = lifecycle[0]
	}
	slices.SortFunc(startup, func(a, b WorldInstance) int { return strings.Compare(string(a.ID), string(b.ID)) })
	for _, instance := range startup {
		owner, _, _ := strings.Cut(string(instance.ID), ":")
		if _, err := h.createWorld(ctx, owner, instance); err != nil {
			return err
		}
	}
	return nil
}

// definitionDimension lowers a supported profile into Dragonfly's dimension type, never a wire ID.
func definitionDimension(settings WorldSettings) (world.Dimension, error) {
	if settings.Generator != GeneratorEmpty && settings.Generator != GeneratorFlat {
		return nil, errors.New("unknown generator")
	}
	switch settings.Dimension {
	case DimensionOverworld:
		return world.Overworld, nil
	case DimensionNether:
		return world.Nether, nil
	case DimensionEnd:
		return world.End, nil
	}
	return nil, errors.New("unknown dimension type")
}

// createWorld opens one registered definition. The lifecycle owner holds manager.mu.
func (h *Host) createWorld(ctx context.Context, owner string, instance WorldInstance) (WorldHandle, error) {
	if err := ctx.Err(); err != nil {
		return WorldHandle{}, err
	}
	def, ok := h.manager.definitions[instance.Definition]
	if !ok || !ownedWorldName(string(instance.ID), owner) || !ownedWorldName(string(def.ID), owner) {
		return WorldHandle{}, errors.New("world authority denied")
	}
	if _, exists := h.worlds.handle(instance.ID); exists {
		return WorldHandle{}, errors.New("instance already open")
	}
	dim, err := definitionDimension(def.Settings)
	if err != nil {
		return WorldHandle{}, err
	}
	conf := world.Config{Log: h.log, Dim: dim, Blocks: world.DefaultBlockRegistry, Entities: entity.DefaultRegistry, Generator: world.NopGenerator{}, Synchronous: h.manager.synchronous}
	if def.Settings.Generator == GeneratorFlat {
		conf.Generator = generator.NewFlatWithRegistry(biome.Plains{}, []world.Block{block.Grass{}, block.Dirt{}, block.Bedrock{}}, world.DefaultBlockRegistry)
	}
	conf.Provider, err = h.instanceProvider(owner, instance, def, conf.Generator.DefaultSpawn(dim))
	if err != nil {
		return WorldHandle{}, err
	}
	w := conf.New()
	owned := ownedWorld{w: w, persistent: def.Settings.Persistent}
	if h.manager.lifecycle != nil {
		h.manager.lifecycle.WorldOpened(w)
	}
	handle, err := h.BindWorld(instance.ID, w)
	if err != nil {
		return WorldHandle{}, errors.Join(err, h.releaseWorld(WorldHandle{ID: instance.ID}, owned))
	}
	h.manager.owned[handle] = owned
	return handle, nil
}

// unloadWorld refuses occupied instances and invalidates handles before closing the provider.
func (h *Host) unloadWorld(ctx context.Context, owner string, handle WorldHandle) error {
	if !ownedWorldName(string(handle.ID), owner) {
		return errors.New("world authority denied")
	}
	owned, ok := h.manager.owned[handle]
	if !ok {
		return ErrStaleWorld
	}
	var result error
	if err := await(ctx, owned.w.Do(func(tx *world.Tx) {
		for range tx.Players() {
			result = errors.New("world has players")
			return
		}
	})); err != nil {
		return err
	}
	if result != nil {
		return result
	}
	return h.releaseWorld(handle, owned)
}

// handle returns the current opening for a stable instance ID.
func (b *worldBindings) handle(id cinnabarext.WorldID) (WorldHandle, bool) {
	b.mu.RLock()
	defer b.mu.RUnlock()
	w := b.byID[id]
	if w == nil {
		return WorldHandle{}, false
	}
	return b.byWorld[w].handle, true
}

// handles lists only the mod's live instance openings in stable order.
func (b *worldBindings) handles(owner string) []WorldHandle {
	b.mu.RLock()
	defer b.mu.RUnlock()
	out := make([]WorldHandle, 0)
	for _, instance := range b.byWorld {
		if ownedWorldName(string(instance.handle.ID), owner) {
			out = append(out, instance.handle)
		}
	}
	slices.SortFunc(out, func(a, b WorldHandle) int { return strings.Compare(string(a.ID), string(b.ID)) })
	return out
}

// closeWorlds shuts down only worlds this host created, after its workers have stopped.
func (h *Host) closeWorlds() error {
	h.manager.mu.Lock()
	defer h.manager.mu.Unlock()
	var errs []error
	for handle, owned := range h.manager.owned {
		if err := h.releaseWorld(handle, owned); err != nil {
			errs = append(errs, fmt.Errorf("close instance %s: %w", handle.ID, err))
		}
	}
	return errors.Join(errs...)
}

// releaseWorld invalidates an opening and releases every resource after transactions have drained.
// A zero generation denotes a world whose binding failed during creation. Callers hold manager.mu.
func (h *Host) releaseWorld(handle WorldHandle, owned ownedWorld) error {
	if handle.Generation != 0 {
		_ = h.worlds.unbind(handle)
		delete(h.manager.owned, handle)
	}
	if h.manager.lifecycle != nil {
		h.manager.lifecycle.WorldClosed(owned.w)
	}
	err := owned.w.Close()
	if !owned.persistent {
		h.store.discardPlacements(handle.ID)
	}
	return err
}
