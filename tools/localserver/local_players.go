package main

import (
	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/google/uuid"
)

// localPlayerProvider applies the launcher's mode when restoring the local host's data.
// This loopback server only hosts the local player and overlapping reconnects.
type localPlayerProvider struct {
	player.Provider
	mode world.GameMode
}

// Load keeps the player's saved data, but the launcher's selection owns their login mode.
// Missing records still use Dragonfly's world defaults.
func (p localPlayerProvider) Load(id uuid.UUID, lookup func(world.Dimension) *world.World) (player.Config, *world.World, error) {
	data, w, err := p.Provider.Load(id, lookup)
	if err == nil {
		data.GameMode = p.mode
	}
	return data, w, err
}

// configureLocalPlayers makes the selected world mode apply to returning local hosts too.
// Nil storage stays unset so Dragonfly can install its no-op provider.
func (s settings) configureLocalPlayers(conf *server.Config) {
	if conf.PlayerProvider == nil {
		return
	}
	mode, _ := s.worldGameMode()
	conf.PlayerProvider = localPlayerProvider{Provider: conf.PlayerProvider, mode: mode}
}
