package main

import (
	"testing"

	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
)

func TestGiveShieldUsesVanillaRegistry(t *testing.T) {
	withPlayer(t, world.GameModeSurvival, func(tx *world.Tx, p *player.Player) {
		runCommand(t, tx, p, "give", "shield")
		stack, _ := p.Inventory().Item(0)
		if _, ok := stack.Item().(item.Shield); !ok || stack.Count() != 1 {
			t.Fatalf("shield = %v, want one registered vanilla shield", stack)
		}
	})
}

func TestGivePreservesPotionMetadataAndRejectsInvalidCount(t *testing.T) {
	withPlayer(t, world.GameModeSurvival, func(tx *world.Tx, p *player.Player) {
		runCommand(t, tx, p, "give", "minecraft:potion 1 21")
		stack, _ := p.Inventory().Item(0)
		name, data := stack.Item().EncodeItem()
		if name != "minecraft:potion" || data != 21 {
			t.Fatalf("potion = %s:%d", name, data)
		}
		runCommand(t, tx, p, "give", "stone -1")
		next, _ := p.Inventory().Item(1)
		if !next.Empty() {
			t.Fatalf("negative count added %v", next)
		}
	})
}
