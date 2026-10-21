package localserver

import (
	"fmt"
	"sync"

	"github.com/df-mc/dragonfly/server/world"
)

// GeneratorFactory builds a world's dimension generators after block registration is complete.
// An empty map keeps Dragonfly's defaults. Nonempty maps must contain every dimension.
// Generators implementing io.Closer are closed when startup fails or the server stops.
type GeneratorFactory func(seed int64) (map[world.Dimension]world.Generator, error)

// GeneratorInfo is one entry in the -list-generators contract.
type GeneratorInfo struct {
	ID    string `json:"id"`
	Label string `json:"label"`
}

type registeredGenerator struct {
	info    GeneratorInfo
	factory GeneratorFactory
}

var generatorRegistry = struct {
	sync.RWMutex
	entries []registeredGenerator
}{}

// RegisterGenerator adds a stable id and display label before Run is called.
// It rejects empty fields, nil factories and duplicate ids without changing the registry.
func RegisterGenerator(id, label string, factory GeneratorFactory) error {
	if id == "" || label == "" || factory == nil {
		return fmt.Errorf("generator id, label and factory are required")
	}
	generatorRegistry.Lock()
	defer generatorRegistry.Unlock()
	for _, entry := range generatorRegistry.entries {
		if entry.info.ID == id {
			return fmt.Errorf("generator %q is already registered", id)
		}
	}
	generatorRegistry.entries = append(generatorRegistry.entries, registeredGenerator{GeneratorInfo{id, label}, factory})
	return nil
}

// generatorList returns an independent catalog in registration order.
func generatorList() []GeneratorInfo {
	generatorRegistry.RLock()
	defer generatorRegistry.RUnlock()
	list := make([]GeneratorInfo, 0, len(generatorRegistry.entries))
	for _, entry := range generatorRegistry.entries {
		list = append(list, entry.info)
	}
	return list
}

// lookupGenerator finds a factory without holding the registry lock while it builds.
func lookupGenerator(id string) (GeneratorFactory, bool) {
	generatorRegistry.RLock()
	defer generatorRegistry.RUnlock()
	for _, entry := range generatorRegistry.entries {
		if entry.info.ID == id {
			return entry.factory, true
		}
	}
	return nil, false
}

// init registers the shipped generators through the same API used by external servers.
func init() {
	for _, entry := range []registeredGenerator{
		{GeneratorInfo{"normal", "Normal (Vanilla)"}, func(seed int64) (map[world.Dimension]world.Generator, error) { return buildGenerators(seed) }},
		{GeneratorInfo{"flat", "Flat"}, func(int64) (map[world.Dimension]world.Generator, error) { return nil, nil }},
	} {
		if err := RegisterGenerator(entry.info.ID, entry.info.Label, entry.factory); err != nil {
			panic(err)
		}
	}
}
