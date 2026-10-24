package experience

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/df-mc/dragonfly/server/world/mcdb"
)

// temporaryProvider retains unloaded terrain until the instance closes.
type temporaryProvider struct {
	world.Provider
	dir string
}

// Close closes the database before removing its instance-lifetime directory.
func (p *temporaryProvider) Close() error {
	return errors.Join(p.Provider.Close(), os.RemoveAll(p.dir))
}

// instanceProvider checks saved identity before opening durable or instance-lifetime storage.
func (h *Host) instanceProvider(owner string, instance WorldInstance, def WorldDefinition, spawn cube.Pos) (world.Provider, error) {
	var dir string
	metadataExists := false
	if h.manager.root != "" {
		_, name, _ := strings.Cut(string(instance.ID), ":")
		dir = filepath.Join(h.manager.root, owner, name)
		old, err := os.ReadFile(filepath.Join(dir, "experience-world.json"))
		switch {
		case err == nil:
			metadataExists = true
			var stored WorldDefinition
			if json.Unmarshal(old, &stored) != nil || stored != def {
				return nil, errors.New("saved instance definition changed; explicit migration required")
			}
		case !errors.Is(err, os.ErrNotExist):
			return nil, err
		}
	}
	if def.Settings.Persistent {
		if dir == "" {
			return nil, errors.New("persistent world directory unavailable")
		}
		if err := os.MkdirAll(dir, 0o755); err != nil {
			return nil, err
		}
		if !metadataExists {
			if err := writeJSONFile(filepath.Join(dir, "experience-world.json"), def); err != nil {
				return nil, err
			}
		}
		return h.openTerrain(dir, spawn)
	}
	dir, err := os.MkdirTemp("", "cinnabar-world-")
	if err != nil {
		return nil, err
	}
	provider, err := h.openTerrain(dir, spawn)
	if err != nil {
		return nil, errors.Join(err, os.RemoveAll(dir))
	}
	h.store.discardPlacements(instance.ID)
	return &temporaryProvider{Provider: provider, dir: dir}, nil
}

// openTerrain initializes a new database's spawn while retaining every saved world's settings.
func (h *Host) openTerrain(dir string, spawn cube.Pos) (world.Provider, error) {
	_, statErr := os.Stat(filepath.Join(dir, "level.dat"))
	if statErr != nil && !errors.Is(statErr, os.ErrNotExist) {
		return nil, statErr
	}
	provider, err := (mcdb.Config{Log: h.log, Blocks: world.DefaultBlockRegistry}).Open(dir)
	if err == nil && errors.Is(statErr, os.ErrNotExist) {
		provider.Settings().Spawn = spawn
	}
	return provider, err
}
