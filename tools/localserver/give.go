package main

import (
	"strings"

	"github.com/df-mc/dragonfly/server/cmd"
	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
)

type giveItem struct {
	Name  string              `cmd:"item"`
	Count cmd.Optional[int]   `cmd:"count"`
	Data  cmd.Optional[int16] `cmd:"data"`
}

// Run resolves registered vanilla items, including metadata variants, without custom replacements.
func (g giveItem) Run(src cmd.Source, o *cmd.Output, _ *world.Tx) {
	p, ok := src.(*player.Player)
	if !ok {
		o.Error("only players can receive items")
		return
	}
	name := g.Name
	if !strings.Contains(name, ":") {
		name = "minecraft:" + name
	}
	it, ok := world.ItemByName(name, g.Data.LoadOr(0))
	if !ok {
		o.Errorf("unknown item %s", name)
		return
	}
	stack := item.NewStack(it, 1)
	count := g.Count.LoadOr(1)
	if count < 1 || count > stack.MaxCount() {
		o.Errorf("count must be between 1 and %d", stack.MaxCount())
		return
	}
	added, err := p.Inventory().AddItem(stack.Grow(count - 1))
	if err != nil {
		o.Error(err)
	}
	o.Printf("Gave %d %s", added, name)
}
