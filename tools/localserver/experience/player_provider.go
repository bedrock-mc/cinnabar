package experience

import (
	"context"
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

// Load provides an unreserved snapshot for callers outside Dragonfly's login lifecycle.
// Dragonfly uses LoadAdmission to retain the destination until spawn or failure.
func (p *instancePlayerProvider) Load(id uuid.UUID, lookup func(world.Dimension) *world.World) (player.Config, *world.World, error) {
	admission, err := p.LoadAdmission(id, lookup)
	if err != nil {
		return player.Config{}, nil, err
	}
	defer admission.Release()
	data, w := admission.Resolve()
	return data, w, nil
}

// LoadAdmission reserves one login's saved destination independently of overlapping logins.
func (p *instancePlayerProvider) LoadAdmission(id uuid.UUID, lookup func(world.Dimension) *world.World) (player.Admission, error) {
	p.mu.Lock()
	defer p.mu.Unlock()
	data, _, err := p.Provider.Load(id, lookup)
	if err != nil {
		return nil, err
	}
	var record savedPlayerLocation
	encoded, err := os.ReadFile(filepath.Join(p.dir, id.String()+".json"))
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		return nil, err
	}
	if err == nil {
		if err := json.Unmarshal(encoded, &record); err != nil {
			return nil, fmt.Errorf("load player location: %w", err)
		}
	}
	admission := &instanceAdmission{data: data, fallback: lookup(world.Overworld), host: p.host}
	if handle, ok := p.host.worlds.handle(record.Instance); ok && !p.host.closed.Load() {
		admission.reservation, admission.w, _ = p.host.worlds.reserve(handle)
	}
	if admission.w != nil {
		admission.data.Position = record.Position.vec()
	}
	if admission.w == nil {
		admission.useFallback()
	}
	return admission, nil
}

// instanceAdmission owns the destination across the handshake and resolves a safe spawn at admission.
type instanceAdmission struct {
	mu          sync.Mutex
	data        player.Config
	w, fallback *world.World
	host        *Host
	reservation *worldReservation
}

// Resolve rechecks the opening and world task admission just before the server uses it.
func (a *instanceAdmission) Resolve() (player.Config, *world.World) {
	a.mu.Lock()
	defer a.mu.Unlock()
	if a.w != nil && a.reservation != nil {
		_, err := a.host.ResolveWorld(a.reservation.handle)
		if err == nil {
			err = a.w.Do(func(*world.Tx) {}).Wait(context.Background())
		}
		if err == nil {
			return a.data, a.w
		}
	}
	a.useFallback()
	return a.data, a.w
}

// Release ends this login's reservation after spawn or failure without touching another login.
func (a *instanceAdmission) Release() {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.reservation.Release()
	a.reservation, a.w = nil, nil
}

// useFallback drops foreign coordinates and reserves the default opening when it is bound.
// The caller holds a.mu or has not published the admission yet.
func (a *instanceAdmission) useFallback() {
	a.reservation.Release()
	a.reservation, a.w = nil, a.fallback
	a.data.Position = a.fallback.Spawn().Vec3Centre()
	if instance, ok := a.host.worlds.of(a.fallback); ok && !a.host.closed.Load() {
		a.reservation, _, _ = a.host.worlds.reserve(instance.handle)
	}
}
