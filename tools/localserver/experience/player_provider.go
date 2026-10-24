package experience

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sync"

	"github.com/cinnabar-mc/cinnabar/tools/localserver/cinnabarext"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/google/uuid"
)

// savedPlayerLocation keeps coordinates and their stable instance in one atomic record.
type savedPlayerLocation struct {
	Instance cinnabarext.WorldID `json:"instance"`
	Position Position            `json:"position"`
}

// instancePlayerProvider adds instance-aware positions to Dragonfly's ordinary player data.
type instancePlayerProvider struct {
	player.Provider
	host *Host
	dir  string
	mu   sync.Mutex
}

// PlayerProvider wraps existing storage before the server starts accepting connections.
// A missing location record migrates safely to the default spawn.
func (h *Host) PlayerProvider(provider player.Provider, dir string) (player.Provider, error) {
	if provider == nil {
		return nil, nil
	}
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return nil, err
	}
	return &instancePlayerProvider{Provider: provider, host: h, dir: dir}, nil
}

// Save pairs the committed position with its instance, independently of the dimension-only provider.
func (p *instancePlayerProvider) Save(id uuid.UUID, data player.Config, w *world.World) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	instance, ok := p.host.worlds.of(w)
	if !ok {
		return errors.New("saving player in an unbound world")
	}
	if err := p.Provider.Save(id, data, w); err != nil {
		return err
	}
	record := savedPlayerLocation{Instance: instance.handle.ID, Position: positionOf(data.Position)}
	encoded, err := json.Marshal(record)
	if err != nil {
		return err
	}
	path := filepath.Join(p.dir, id.String()+".json")
	if err := os.WriteFile(path+".tmp", encoded, 0o600); err != nil {
		return err
	}
	return os.Rename(path+".tmp", path)
}

// Load resolves the saved instance and position together, falling back to the default spawn.
func (p *instancePlayerProvider) Load(id uuid.UUID, lookup func(world.Dimension) *world.World) (player.Config, *world.World, error) {
	p.mu.Lock()
	defer p.mu.Unlock()
	data, w, err := p.Provider.Load(id, lookup)
	if err != nil {
		return data, w, err
	}
	var record savedPlayerLocation
	encoded, err := os.ReadFile(filepath.Join(p.dir, id.String()+".json"))
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		return data, nil, err
	}
	if err == nil {
		if err := json.Unmarshal(encoded, &record); err != nil {
			return data, nil, fmt.Errorf("load player location: %w", err)
		}
	}
	w, position := p.host.resolvePlayerLocation(record, lookup(world.Overworld))
	data.Position = position.vec()
	return data, w, nil
}

// resolvePlayerLocation uses a stable instance only while it is open; foreign coordinates never reach a default world.
func (h *Host) resolvePlayerLocation(record savedPlayerLocation, fallback *world.World) (*world.World, Position) {
	if handle, ok := h.worlds.handle(record.Instance); ok {
		if w, err := h.ResolveWorld(handle); err == nil {
			return w, record.Position
		}
	}
	return fallback, positionOf(fallback.Spawn().Vec3Centre())
}
