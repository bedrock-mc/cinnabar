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
		out.Errorf("no items match %q", f.Filter)
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
	name, data := items[index].EncodeItem()
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

// fixtureItem supplies a renderable identity without adding a gameplay implementation.
type fixtureItem struct {
	name     string
	maxCount int
}

// EncodeItem returns the advertised vanilla identity with its default metadata.
func (i fixtureItem) EncodeItem() (string, int16) { return i.name, 0 }

// MaxCount keeps fixture stacks within the runtime dictionary's stack limit.
func (i fixtureItem) MaxCount() int { return i.maxCount }

// fixtureItems combines registered variants with advertised identities in stable name and metadata order.
func fixtureItems(registered []world.Item, filter string) []world.Item {
	implemented := make(map[string]bool, len(registered))
	for _, entry := range registered {
		name, metadata := entry.EncodeItem()
		if metadata == 0 {
			implemented[name] = true
		}
	}
	for name, entry := range world.VanillaItemEntries() {
		if !implemented[name] {
			registered = append(registered, fixtureItem{name: name, maxCount: entry.MaxStackSize})
		}
	}
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
