package main

import (
	"slices"
	"testing"

	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
)

func TestFixtureItemsOrderDoesNotDependOnRegistrationOrder(t *testing.T) {
	registered := world.Items()
	want := fixtureItems(registered, "potion")
	slices.Reverse(registered)
	got := fixtureItems(registered, "POTION")
	if len(got) < 2 || len(got) != len(want) {
		t.Fatalf("filtered items = %d, want %d potion variants", len(got), len(want))
	}
	for i := range got {
		name, data := got[i].EncodeItem()
		wantName, wantData := want[i].EncodeItem()
		if name != wantName || data != wantData {
			t.Fatalf("item %d = %s:%d, want %s:%d", i, name, data, wantName, wantData)
		}
		if i > 0 {
			previous, metadata := got[i-1].EncodeItem()
			if name < previous || (name == previous && data <= metadata) {
				t.Fatalf("items are not ordered by unique identity at %s:%d", name, data)
			}
		}
	}
}

func TestItemFixturePopulatesHandsAndHotbarAndWraps(t *testing.T) {
	withPlayer(t, world.GameModeCreative, func(tx *world.Tx, p *player.Player) {
		_ = p.SetHeldSlot(5)
		registered := fixtureItems(world.Items(), "potion")
		runCommand(t, tx, p, "fixture_items", "potion 1")
		for slot := range 9 {
			stack, _ := p.Inventory().Item(slot)
			assertFixtureItem(t, stack, registered[(slot+1)%len(registered)])
		}
		main, off := p.HeldItems()
		assertFixtureItem(t, main, registered[1])
		assertFixtureItem(t, off, registered[1])
		runCommand(t, tx, p, "fixture_items", "shield 99")
		shield, _ := world.ItemByName("minecraft:shield", 0)
		main, off = p.HeldItems()
		assertFixtureItem(t, main, shield)
		assertFixtureItem(t, off, shield)
	})
}

func TestInvalidItemFixtureDoesNotChangeInventory(t *testing.T) {
	withPlayer(t, world.GameModeCreative, func(tx *world.Tx, p *player.Player) {
		shield, _ := world.ItemByName("minecraft:shield", 0)
		p.SetHeldItems(item.NewStack(shield, 1), item.NewStack(shield, 1))
		for _, args := range []string{"potion -1", "no-such-item"} {
			runCommand(t, tx, p, "fixture_items", args)
			main, off := p.HeldItems()
			assertFixtureItem(t, main, shield)
			assertFixtureItem(t, off, shield)
		}
	})
}

// assertFixtureItem checks the complete registry identity and the fixture's single-item count.
func assertFixtureItem(t *testing.T, stack item.Stack, want world.Item) {
	t.Helper()
	name, data := stack.Item().EncodeItem()
	wantName, wantData := want.EncodeItem()
	if name != wantName || data != wantData || stack.Count() != 1 {
		t.Fatalf("item = %s:%d x%d, want %s:%d x1", name, data, stack.Count(), wantName, wantData)
	}
}
