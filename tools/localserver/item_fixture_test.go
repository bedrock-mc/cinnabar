package main

import (
	"reflect"
	"slices"
	"testing"

	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
)

func TestFixtureItemsIncludeEveryAdvertisedIdentity(t *testing.T) {
	selected := fixtureItems(world.Items(), "*")
	defaults := make(map[string]world.Item)
	for _, entry := range selected {
		name, metadata := entry.EncodeItem()
		if metadata == 0 {
			if _, duplicate := defaults[name]; duplicate {
				t.Fatalf("duplicate default identity %s", name)
			}
			defaults[name] = entry
		}
	}
	for name, advertised := range world.VanillaItemEntries() {
		entry, ok := defaults[name]
		if !ok {
			t.Errorf("advertised item %s is missing from the fixture", name)
			continue
		}
		runtimeID, metadata, ok := world.ItemRuntimeID(entry)
		if !ok || runtimeID != advertised.RuntimeID || metadata != 0 {
			t.Errorf("fixture item %s has runtime identity %d:%d, registered=%t", name, runtimeID, metadata, ok)
		}
		if _, fallback := entry.(fixtureItem); fallback && item.NewStack(entry, 1).MaxCount() != advertised.MaxStackSize {
			t.Errorf("fixture item %s does not preserve its advertised stack limit", name)
		}
	}
}

func TestFixtureItemsPreserveRegisteredVariants(t *testing.T) {
	registered := world.Items()
	selected := fixtureItems(registered, "*")
	for _, entry := range registered {
		name, metadata := entry.EncodeItem()
		index := slices.IndexFunc(selected, func(candidate world.Item) bool {
			candidateName, candidateMetadata := candidate.EncodeItem()
			return candidateName == name && candidateMetadata == metadata
		})
		if index < 0 || !reflect.DeepEqual(selected[index], entry) {
			t.Errorf("registered variant %s:%d was replaced", name, metadata)
		}
	}
}

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
		p.SetHeldItems(main, item.NewStack(registered[1], 1))
		_, off = p.HeldItems()
		assertFixtureItem(t, off, shield)
	})
}

// Air is registered but its stacks are empty, so selecting it must not dereference the stack's item.
func TestItemFixtureSelectsAir(t *testing.T) {
	withPlayer(t, world.GameModeCreative, func(tx *world.Tx, p *player.Player) {
		runCommand(t, tx, p, "fixture_items", "minecraft:air")
		main, off := p.HeldItems()
		if !main.Empty() || !off.Empty() {
			t.Fatalf("held items = %v, %v; want empty air stacks", main, off)
		}
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
	if stack.Empty() {
		t.Fatal("fixture item is empty")
	}
	name, data := stack.Item().EncodeItem()
	wantName, wantData := want.EncodeItem()
	if name != wantName || data != wantData || stack.Count() != 1 {
		t.Fatalf("item = %s:%d x%d, want %s:%d x1", name, data, stack.Count(), wantName, wantData)
	}
}
