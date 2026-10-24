package main

import (
	"cmp"
	"slices"
	"strings"

	"github.com/df-mc/dragonfly/server/cmd"
	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
)

type itemFixture struct {
	Filter string            `cmd:"filter"`
	Index  cmd.Optional[int] `cmd:"index"`
}

// Run selects a registry item in both hands and fills the hotbar in stable registry order.
func (f itemFixture) Run(src cmd.Source, out *cmd.Output, _ *world.Tx) {
	p, ok := src.(*player.Player)
	if !ok {
		out.Error("only players can run the item fixture")
		return
	}
	index := f.Index.LoadOr(0)
	if index < 0 {
		out.Error("index must be nonnegative")
		return
	}
	items := fixtureItems(world.Items(), f.Filter)
	if len(items) == 0 {
		out.Errorf("no registered items match %q", f.Filter)
		return
	}
	index %= len(items)
	_ = p.SetHeldSlot(0)
	for slot := range 9 {
		_ = p.Inventory().SetItem(slot, item.NewStack(items[(index+slot)%len(items)], 1))
	}
	stack := item.NewStack(items[index], 1)
	offhand := p.Data().OffHand
	offhand.SlotValidatorFunc(func(item.Stack, int) bool { return true })
	p.SetHeldItems(stack, stack)
	offhand.SlotValidatorFunc(fixtureOffhandAllowed)
	name, data := stack.Item().EncodeItem()
	out.Printf("Item %d/%d: %s data=%d; next index=%d", index, len(items), name, data, (index+1)%len(items))
}

// fixtureOffhandAllowed restores ordinary offhand admission after server-driven fixture injection.
func fixtureOffhandAllowed(stack item.Stack, _ int) bool {
	if stack.Empty() {
		return true
	}
	allowed, ok := stack.Item().(item.OffHand)
	return ok && allowed.OffHand()
}

// fixtureItems filters registered identities and sorts by name and metadata for repeatable captures.
func fixtureItems(registered []world.Item, filter string) []world.Item {
	filter = strings.ToLower(filter)
	var selected []world.Item
	for _, entry := range registered {
		name, _ := entry.EncodeItem()
		if filter == "*" || strings.Contains(name, filter) {
			selected = append(selected, entry)
		}
	}
	slices.SortFunc(selected, func(a, b world.Item) int {
		an, ad := a.EncodeItem()
		bn, bd := b.EncodeItem()
		if order := cmp.Compare(an, bn); order != 0 {
			return order
		}
		return cmp.Compare(ad, bd)
	})
	return selected
}
