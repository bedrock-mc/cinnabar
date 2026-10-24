package main

import (
	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
	"github.com/google/uuid"

	"github.com/cinnabar-mc/cinnabar/tools/localserver/experience"
)

// localExperienceHost receives session changes and entity observations from the local player.
type localExperienceHost interface {
	TeleportPlayer(*player.Player, experience.Position) bool
	PlayerDisconnected(*player.Player)
	EntityInteraction(*player.Player, world.Entity, bool)
}

// localPlayerHandler owns temporary flight controls and clears Experience focus on quit.
// Its movement handler accepts client positions, including large flight deltas.
type localPlayerHandler struct {
	player.NopHandler
	host   localExperienceHost
	flight flySpeedState
}

// HandleItemUseOnEntity forwards accepted entity interactions to the installed runtime.
func (h *localPlayerHandler) HandleItemUseOnEntity(ctx *player.Context, e world.Entity) {
	if h.host != nil {
		h.host.EntityInteraction(ctx.Player(), e, false)
	}
}

// HandleAttackEntity observes the attack before the framework applies item and damage effects.
func (h *localPlayerHandler) HandleAttackEntity(ctx *player.Context, e world.Entity, _, _ *float64, _ *bool) {
	if h.host != nil {
		h.host.EntityInteraction(ctx.Player(), e, true)
	}
}

// HandleTeleport applies authoritative teleports through the host's session fence.
func (h *localPlayerHandler) HandleTeleport(ctx *player.Context, pos mgl64.Vec3) {
	if h.host != nil && h.host.TeleportPlayer(ctx.Player(), experience.Position{X: pos.X(), Y: pos.Y(), Z: pos.Z()}) {
		ctx.Cancel()
	}
}

// HandleQuit forgets this player's focus when the server hosts Experiences.
func (h *localPlayerHandler) HandleQuit(p *player.Player) {
	if h.host != nil {
		h.host.PlayerDisconnected(p)
	}
}

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
