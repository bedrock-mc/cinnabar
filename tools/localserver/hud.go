package localserver

import (
	"strings"

	"github.com/df-mc/dragonfly/server/cmd"
	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
)

// init registers a deterministic inventory and equipment fixture for HUD comparisons.
func init() {
	cmd.Register(cmd.New("hudfixture", "Fills inventory and armour for HUD checks", nil, hudFixture{}))
	cmd.Register(cmd.New("glyphfixture", "Shows original bitmap spacing probes", nil, glyphFixture{}))
}

type glyphFixture struct{}

// Run publishes fixed strings so the same original glyph page can be measured in both clients.
func (glyphFixture) Run(src cmd.Source, o *cmd.Output, _ *world.Tx) {
	p, ok := src.(*player.Player)
	if !ok {
		o.Error("Only players can use this fixture")
		return
	}
	for _, probe := range []struct {
		Name  string
		Glyph rune
	}{
		{"Solid left", '\ue100'}, {"Solid inset", '\ue101'}, {"Blank anchor", '\ue102'},
		{"Solid full", '\ue103'}, {"Inset anchor", '\ue104'},
	} {
		p.Message(probe.Name + ": A" + strings.Repeat(string(probe.Glyph), 9) + "Z")
	}
}

type hudFixture struct {
	Clear cmd.Optional[bool] `cmd:"clear"`
}

// Run changes only the testing player's slots, through the inventory's normal packet owner.
func (f hudFixture) Run(src cmd.Source, o *cmd.Output, _ *world.Tx) {
	p, ok := src.(*player.Player)
	if !ok {
		o.Error("Only players can use this fixture")
		return
	}
	p.Inventory().Clear()
	p.Armour().Clear()
	if f.Clear.LoadOr(false) {
		o.Print("HUD fixture cleared")
		return
	}
	for _, slot := range []struct {
		Index int
		Name  string
		Count int
	}{
		{0, "minecraft:diamond", 32}, {8, "minecraft:apple", 1},
		{9, "minecraft:gold_ingot", 64}, {19, "minecraft:arrow", 12},
		{35, "minecraft:iron_ingot", 7},
	} {
		it, found := world.ItemByName(slot.Name, 0)
		if !found {
			o.Errorf("Fixture item unavailable: %s", slot.Name)
			return
		}
		if err := p.Inventory().SetItem(slot.Index, item.NewStack(it, slot.Count)); err != nil {
			o.Error(err)
			return
		}
	}
	p.Armour().Set(
		item.NewStack(item.Helmet{Tier: item.ArmourTierDiamond{}}, 1).Damage(100),
		item.NewStack(item.Chestplate{Tier: item.ArmourTierDiamond{}}, 1).Damage(100),
		item.NewStack(item.Leggings{Tier: item.ArmourTierDiamond{}}, 1).Damage(100),
		item.NewStack(item.Boots{Tier: item.ArmourTierDiamond{}}, 1).Damage(100),
	)
	o.Print("HUD fixture filled")
}
