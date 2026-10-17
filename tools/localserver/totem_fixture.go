package main

import (
	"github.com/df-mc/dragonfly/server/cmd"
	"github.com/df-mc/dragonfly/server/entity"
	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
)

type totemFixture struct{}

// Run applies real server damage only when a damageable player holds a protective totem.
func (totemFixture) Run(src cmd.Source, out *cmd.Output, _ *world.Tx) {
	p, ok := src.(*player.Player)
	if !ok {
		out.Error("only players can run the totem fixture")
		return
	}
	if !p.GameMode().AllowsTakingDamage() || p.Dead() {
		out.Error("the totem fixture requires a living player in a damageable game mode")
		return
	}
	hand, offhand := p.HeldItems()
	_, mainTotem := hand.Item().(item.Totem)
	_, offTotem := offhand.Item().(item.Totem)
	if !mainTotem && !offTotem {
		out.Error("hold a totem before running the fixture")
		return
	}
	p.Hurt(1000, entity.FallDamageSource{})
	out.Print("Applied server damage; attack immunity may defer another activation")
}
