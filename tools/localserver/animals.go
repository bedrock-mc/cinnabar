package main

import (
	"github.com/df-mc/dragonfly/server/entity"
	"github.com/df-mc/dragonfly/server/world"
)

// configureAnimals enables owner-driven passive population in normal saved
// worlds. Terrain and synthetic rendering fixtures keep their existing contents.
func (s settings) configureAnimals(w *world.World) {
	if s.generator != "normal" || s.terrainFixture || s.opaqueOverdraw {
		return
	}
	w.Handle(entity.NewAnimalSpawner(w.Handler(), uint64(s.seed)))
}
