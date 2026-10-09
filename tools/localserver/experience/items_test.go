package experience

import (
	"context"
	"encoding/hex"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/df-mc/dragonfly/server/entity"
	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/item/creative"
	"github.com/df-mc/dragonfly/server/world"
)

// The probe's item, one to a stack, and its behavior that reads the held stack, puts a cell
// with data 07 in slot 0 and drops two stone above the block.
const (
	probeCell   = "probe:cell"
	probeStacks = 27
)

// cell is a stack of the Experience exp's cell with data, or none.
func cellStack(t *testing.T, exp string, data []byte) item.Stack {
	t.Helper()
	it, ok := registered(t).LookupItem(exp + ":cell")
	if !ok {
		t.Fatalf("no item %s:cell", exp)
	}
	s := item.NewStack(it, 1)
	if data != nil {
		s = s.WithValue(itemDataKey, data)
	}
	return s
}

// The probe's item is a custom item of the finalized registries, one to a stack, with its icon,
// in the probe's creative group; "other"'s copy is another item.
func TestItemsRegistered(t *testing.T) {
	reg := registered(t)
	cell, ok := reg.LookupItem(probeCell)
	if !ok {
		t.Fatalf("LookupItem(%q) found nothing", probeCell)
	}
	if got, ok := world.ItemByName(probeCell, 0); !ok || got != world.Item(cell) {
		t.Fatalf("ItemByName(%q) = %v, %v", probeCell, got, ok)
	}
	if cell.MaxCount() != 1 || cell.Name() != "Probe Cell" || cell.Texture() == nil {
		t.Errorf("cell stacks to %d, is named %q, has texture %v", cell.MaxCount(), cell.Name(), cell.Texture() != nil)
	}
	if !slices.ContainsFunc(creative.Items(), func(i creative.Item) bool {
		return i.Group == "probe" && i.Stack.Item() == world.Item(cell)
	}) {
		t.Error("the cell is not in the probe's creative group")
	}
	if other, ok := reg.LookupItem("other:cell"); !ok || other == cell {
		t.Errorf("other:cell = %v, %v; want an item of its own", other, ok)
	}
}

// The adapter checks an Experience's items again: its namespace, distinct from its blocks,
// 1 to maxStackSize to a stack, and an icon that decodes.
func TestItemRules(t *testing.T) {
	icon := writeTexture(t, "icon.png", 16, 16)
	blocks := []BlockDef{testBlock("rules:block", icon)}
	for _, c := range []struct {
		name string
		edit func(*ItemDef)
		want string
	}{
		{"the item", func(*ItemDef) {}, ""},
		{"a foreign namespace", func(i *ItemDef) { i.ID = "other:cell" }, "outside namespace"},
		{"a block's id", func(i *ItemDef) { i.ID = "rules:block" }, "is also a block"},
		{"a stack of 0", func(i *ItemDef) { i.MaxStack = 0 }, "1 to 64"},
		{"a stack of 65", func(i *ItemDef) { i.MaxStack = maxStackSize + 1 }, "1 to 64"},
		{"an icon that is not a PNG", func(i *ItemDef) { i.Icon = icon + ".missing" }, "icon"},
		{"an empty display name", func(i *ItemDef) { i.DisplayName = "" }, "display name"},
	} {
		t.Run(c.name, func(t *testing.T) {
			def := ItemDef{ID: "rules:cell", DisplayName: "Cell", Icon: icon, MaxStack: 1}
			c.edit(&def)
			_, err := newItemType("rules", def, blocks, newAssetCache())
			switch {
			case c.want == "" && err != nil:
				t.Fatalf("refused: %v", err)
			case c.want != "" && (err == nil || !strings.Contains(err.Error(), c.want)):
				t.Fatalf("%v, want %q", err, c.want)
			}
		})
	}
}

// A snapshot stack carries the id, metadata, count and most; data only on the Experience's own
// item; and plain only when it carries nothing else.
func TestStacksAsTheGuestSeesThem(t *testing.T) {
	registered(t)
	stone, _ := world.ItemByName("minecraft:stone", 0)
	sword := item.Sword{Tier: item.ToolTierDiamond}
	for _, c := range []struct {
		name string
		s    item.Stack
		want ItemStack
	}{
		{"stone", item.NewStack(stone, 32), ItemStack{ID: "minecraft:stone", Count: 32, MaxCount: 64, Plain: true}},
		{"a named stack", item.NewStack(stone, 1).WithCustomName("Rock"), ItemStack{ID: "minecraft:stone", Count: 1, MaxCount: 64}},
		{"lore", item.NewStack(stone, 1).WithLore("old"), ItemStack{ID: "minecraft:stone", Count: 1, MaxCount: 64}},
		{"a worn sword", item.NewStack(sword, 1).Damage(3), ItemStack{ID: "minecraft:diamond_sword", Count: 1, MaxCount: 1}},
		{"a fresh sword", item.NewStack(sword, 1), ItemStack{ID: "minecraft:diamond_sword", Count: 1, MaxCount: 1, Plain: true}},
		{"its own cell", cellStack(t, "probe", []byte{7}), ItemStack{ID: probeCell, Count: 1, MaxCount: 1, Data: ptr("07"), Plain: true}},
		{"its own cell without data", cellStack(t, "probe", nil), ItemStack{ID: probeCell, Count: 1, MaxCount: 1, Plain: true}},
		{"another Experience's cell", cellStack(t, "other", []byte{7}), ItemStack{ID: "other:cell", Count: 1, MaxCount: 1}},
	} {
		t.Run(c.name, func(t *testing.T) {
			if got := stackOf("probe", c.s); jsonOf(got) != jsonOf(c.want) {
				t.Errorf("stackOf = %s, want %s", jsonOf(got), jsonOf(c.want))
			}
		})
	}
}

func ptr(s string) *string { return &s }

// The adapter makes the guest's stacks again as the runtime checked them: its own items with
// data, its blocks and the server's items without, each within its most.
func TestMakingStacks(t *testing.T) {
	reg := registered(t)
	data := hex.EncodeToString([]byte{1, 2})
	cell, err := reg.makeStack("probe", NewStack{ID: probeCell, Count: 1, Data: &data})
	if v, _ := cell.Value(itemDataKey); err != nil || cell.Count() != 1 || string(v.([]byte)) != "\x01\x02" {
		t.Fatalf("own cell = %v (data %v), %v", cell, v, err)
	}
	if s, err := reg.makeStack("probe", NewStack{ID: "minecraft:ender_pearl", Count: 16}); err != nil || s.Count() != 16 {
		t.Fatalf("pearls = %v, %v", s, err)
	}
	if s, err := reg.makeStack("probe", NewStack{ID: probeCounter, Count: 64}); err != nil || s.Count() != 64 {
		t.Fatalf("counters = %v, %v", s, err)
	}
	big := hex.EncodeToString(make([]byte, maxItemDataBytes+1))
	for _, bad := range []NewStack{
		{ID: "other:cell", Count: 1},
		{ID: "minecraft:nonsense", Count: 1},
		{ID: "minecraft:ender_pearl", Count: 17},
		{ID: probeCell, Count: 2},
		{ID: "minecraft:stone", Count: 1, Data: &data},
		{ID: probeCell, Count: 1, Data: &big},
		{ID: "minecraft:stone", Count: 0},
	} {
		if s, err := reg.makeStack("probe", bad); err == nil {
			t.Errorf("made %v from %s", s, jsonOf(bad))
		}
	}
}

// Vanilla variants preserve their metadata, and unknown variants never fall back to variant zero.
func TestMakingStacksChecksVanillaMetadata(t *testing.T) {
	reg := registered(t)
	for _, metadata := range []uint16{0, 1} {
		s, err := reg.makeStack("probe", NewStack{ID: "minecraft:potion", Metadata: metadata, Count: 1})
		if err != nil {
			t.Fatal(err)
		}
		if got := stackOf("probe", s); got.Metadata != metadata {
			t.Fatalf("requested potion %d, made %d", metadata, got.Metadata)
		}
	}
	if s, err := reg.makeStack("probe", NewStack{ID: "minecraft:potion", Metadata: 65535, Count: 1}); err == nil {
		t.Fatalf("made %v for an unsupported potion variant", s)
	}
}

// The load catalogue contains each supported vanilla metadata variant, without fallback variants.
func TestServerItemsListMetadata(t *testing.T) {
	registered(t)
	items := serverItems()
	for _, metadata := range []uint16{0, 1} {
		if !slices.ContainsFunc(items, func(i ServerItem) bool {
			return i.ID == "minecraft:potion" && i.Metadata == metadata && i.MaxCount == 1 && !i.OffHand
		}) {
			t.Fatalf("the load catalogue lacks potion variant %d", metadata)
		}
	}
	if !slices.ContainsFunc(items, func(i ServerItem) bool {
		return i.ID == "minecraft:arrow" && i.Metadata == 0 && i.OffHand
	}) {
		t.Fatal("the load catalogue lacks offhand eligibility for arrows")
	}
	if slices.ContainsFunc(items, func(i ServerItem) bool {
		return i.ID == "minecraft:potion" && i.Metadata == 65535
	}) {
		t.Fatal("the load catalogue lists an unsupported potion variant")
	}
}

// Own-item data survives what keeps a stack: the disk form a restart reads back, the network
// form a client echoes, and a copy of another count, as a creative copy is.
func TestItemDataSurvives(t *testing.T) {
	s := cellStack(t, "probe", []byte{7, 8})
	for name, back := range map[string]item.Stack{
		"disk":    item.ReadNBT(item.WriteNBT(s, true), nil),
		"network": item.ReadNBT(item.WriteNBT(s, false), &s),
		"copy":    s.Grow(1).Grow(-1),
	} {
		if v, ok := back.Value(itemDataKey); !ok || string(v.([]byte)) != "\x07\x08" || !back.Comparable(s) {
			t.Errorf("%s: data %v, %v; comparable %v", name, v, ok, back.Comparable(s))
		}
	}
}

// The probe's x=27 crosses the runtime and the adapter: it reads the held stone, puts its cell
// with data in slot 0 and drops two stone above the block.
func TestItemsRoundTrip(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	stone, _ := world.ItemByName("minecraft:stone", 0)
	at := probePos(probeStacks)
	f.place(probeCounter, at, nil)
	f.do(func(tx *world.Tx) { _ = f.player(tx).Inventory().SetItem(0, item.NewStack(stone, 32)) })
	f.activate(at)
	f.waitTells(5*time.Second, "held minecraft:stone 32/64 plain true cell ok drop ok")
	f.do(func(tx *world.Tx) {
		got, _ := f.player(tx).Inventory().Item(0)
		if got.Count() != 1 || got.Item() != world.Item(cellStack(t, "probe", nil).Item()) {
			t.Errorf("slot 0 holds %v, want the probe's cell", got)
		}
		if v, _ := got.Value(itemDataKey); string(v.([]byte)) != "\x07" {
			t.Errorf("the cell's data is %v", v)
		}
		dropped := 0
		for e := range tx.Entities() {
			if e.H().Type() == entity.ItemType {
				dropped++
			}
		}
		if dropped != 1 {
			t.Errorf("%d item entities, want 1", dropped)
		}
	})
}

// A staged slot that changed after the snapshot discards the whole result; foreign and
// over-count stacks are refused.
func TestCommitChecksSlots(t *testing.T) {
	log, logs := testLog(t)
	sup, _ := startFake(t, "ok", log, startOptions{})
	f := newIdleFixture(t, log, map[string]*Supervisor{"probe": sup})
	t.Cleanup(func() { f.host.Close() })
	at := probePos(probeStacks)
	f.place(probeCounter, at, nil)
	d := f.host.dispatchers["probe"]
	ev := event{w: f.w, dim: dimension{num: 0, id: "overworld"}, anchor: at.cube(), actor: f.actor, call: Call{
		Interact: &InteractCall{Player: f.actorID(), Pos: at, Face: FaceUp},
	}}
	snap := f.snapshotOf(ev.call, at)
	if snap.req.Inventory == nil || len(snap.req.Inventory.Slots) != inventorySlots {
		t.Fatalf("an interaction's inventory = %+v", snap.req.Inventory)
	}
	for _, c := range []struct {
		op   Op
		want string
	}{
		{Op{SetSlot: &SetSlotOp{Slot: 0, Stack: &NewStack{ID: "other:cell", Count: 1}}}, "other:cell"},
		{Op{SetSlot: &SetSlotOp{Slot: 0, Stack: &NewStack{ID: "minecraft:stone", Count: 65}}}, "minecraft:stone"},
		{Op{SetSlot: &SetSlotOp{Slot: inventorySlots, Stack: nil}}, "slot"},
		{Op{DropItem: &DropItemOp{Pos: BlockPos{X: at.X, Y: at.Y + 2}, Stack: NewStack{ID: "minecraft:stone", Count: 1}}}, "outside the write scope"},
	} {
		f.host.commit(context.Background(), d, ev, snap, []Op{c.op})
		waitForRecord(t, logs, func(r map[string]any) bool {
			err, _ := r["error"].(string)
			return r["msg"] == "invalid result discarded" && strings.Contains(err, c.want)
		})
	}
	stone, _ := world.ItemByName("minecraft:stone", 0)
	f.do(func(tx *world.Tx) { _ = f.player(tx).Inventory().SetItem(0, item.NewStack(stone, 5)) })
	f.host.commit(context.Background(), d, ev, snap, []Op{{SetSlot: &SetSlotOp{Slot: 0}}})
	waitStale(t, logs, "slot 0")
	f.do(func(tx *world.Tx) {
		if got, _ := f.player(tx).Inventory().Item(0); got.Count() != 5 {
			t.Errorf("a stale result emptied slot 0: %v", got)
		}
	})
}

// An ineligible offhand write rejects the whole callback before its source slot is cleared.
func TestCommitRefusesIneligibleOffhand(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startFake(t, "ok", log, startOptions{})
	f := newIdleFixture(t, log, map[string]*Supervisor{"probe": sup})
	t.Cleanup(func() { f.host.Close() })
	at := probePos(probeStacks)
	f.place(probeCounter, at, nil)
	source := cellStack(t, "probe", []byte{7})
	f.do(func(tx *world.Tx) { _ = f.player(tx).Inventory().SetItem(0, source) })
	call := Call{Interact: &InteractCall{Player: f.actorID(), Pos: at, Face: FaceUp}}
	ev := event{w: f.w, dim: dimension{num: 0, id: "overworld"}, anchor: at.cube(), actor: f.actor, call: call}
	snap := f.snapshotOf(call, at)
	f.host.commit(context.Background(), f.host.dispatchers["probe"], ev, snap, []Op{
		{SetSlot: &SetSlotOp{Slot: 0}},
		{SetSlot: &SetSlotOp{Slot: inventorySlots - 1, Stack: &NewStack{ID: probeCell, Count: 1}}},
	})
	f.do(func(tx *world.Tx) {
		got, _ := f.player(tx).Inventory().Item(0)
		if !got.Comparable(source) || got.Count() != source.Count() {
			t.Errorf("the rejected offhand move changed its source: %v", got)
		}
		if _, offhand := f.player(tx).HeldItems(); !offhand.Empty() {
			t.Errorf("an ineligible item reached the offhand: %v", offhand)
		}
	})
	arrow, ok := world.ItemByName("minecraft:arrow", 0)
	if !ok {
		t.Fatal("no registered arrow")
	}
	f.do(func(tx *world.Tx) { _ = f.player(tx).Inventory().SetItem(0, item.NewStack(arrow, 5)) })
	snap = f.snapshotOf(call, at)
	f.host.commit(context.Background(), f.host.dispatchers["probe"], ev, snap, []Op{
		{SetSlot: &SetSlotOp{Slot: 0}},
		{SetSlot: &SetSlotOp{Slot: inventorySlots - 1, Stack: &NewStack{ID: "minecraft:arrow", Count: 5}}},
	})
	f.do(func(tx *world.Tx) {
		source, _ := f.player(tx).Inventory().Item(0)
		_, offhand := f.player(tx).HeldItems()
		if !source.Empty() || offhand.Count() != 5 || offhand.Item() != arrow {
			t.Errorf("eligible move: source %v, offhand %v", source, offhand)
		}
	})
	snap = f.snapshotOf(call, at)
	f.host.commit(context.Background(), f.host.dispatchers["probe"], ev, snap, []Op{{SetSlot: &SetSlotOp{Slot: inventorySlots - 1}}})
	f.do(func(tx *world.Tx) {
		if _, offhand := f.player(tx).HeldItems(); !offhand.Empty() {
			t.Errorf("offhand clear retained %v", offhand)
		}
	})
}

// Load-time plain flags match fresh stacks made by the adapter, including NBT-bearing blocks.
func TestServerItemsCarryPlainClassification(t *testing.T) {
	reg := registered(t)
	catalog := serverItems()
	for _, id := range []string{"minecraft:stone", "minecraft:chest"} {
		i := slices.IndexFunc(catalog, func(i ServerItem) bool { return i.ID == id && i.Metadata == 0 })
		if i < 0 {
			t.Fatalf("no catalogue entry for %s", id)
		}
		stack, err := reg.makeStack("probe", NewStack{ID: id, Count: 1})
		if err != nil {
			t.Fatal(err)
		}
		if got := catalog[i].Plain; got != stackOf("probe", stack).Plain {
			t.Errorf("%s catalogue plain %v disagrees with its fresh stack", id, got)
		}
	}
}

// Empty items are not creatable stacks or entries in the server catalogue.
func TestServerCatalogueOmitsEmptyItems(t *testing.T) {
	reg := registered(t)
	if stack, err := reg.makeStack("probe", NewStack{ID: airID, Count: 1}); err == nil {
		t.Errorf("made an empty item stack: %v", stack)
	}
	if slices.ContainsFunc(serverItems(), func(i ServerItem) bool { return i.ID == airID }) {
		t.Error("the server catalogue advertises an empty item")
	}
}
