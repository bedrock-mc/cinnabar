package experience

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"

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
func (h *Host) instanceProvider(owner string, instance WorldInstance, def WorldDefinition) (world.Provider, error) {
	var dir string
	if h.manager.root != "" {
		_, name, _ := strings.Cut(string(instance.ID), ":")
		dir = filepath.Join(h.manager.root, owner, name)
		old, err := os.ReadFile(filepath.Join(dir, "experience-world.json"))
		switch {
		case err == nil:
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
		raw, err := json.Marshal(def)
		if err != nil {
			return nil, err
		}
		if err := os.WriteFile(filepath.Join(dir, "experience-world.json"), raw, 0o600); err != nil {
			return nil, err
		}
		return (mcdb.Config{Log: h.log, Blocks: world.DefaultBlockRegistry}).Open(dir)
	}
	dir, err := os.MkdirTemp("", "cinnabar-world-")
	if err != nil {
		return nil, err
	}
	provider, err := (mcdb.Config{Log: h.log, Blocks: world.DefaultBlockRegistry}).Open(dir)
	if err != nil {
		return nil, errors.Join(err, os.RemoveAll(dir))
	}
	return &temporaryProvider{Provider: provider, dir: dir}, nil
}
