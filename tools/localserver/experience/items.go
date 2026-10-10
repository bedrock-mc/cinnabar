package experience

import (
	"cmp"
	"encoding/hex"
	"errors"
	"fmt"
	"image"
	"slices"
	"strings"
	"unicode"

	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/item/category"
	"github.com/df-mc/dragonfly/server/item/inventory"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
)

// itemDataKey is the stack value that holds an Experience's own data on one of its items, as
// []byte. Like every stack value it is saved with the stack and reaches clients in its NBT, as
// AE2's cell contents do; guests decode it as untrusted.
const itemDataKey = "cinnabar:experience_data"

// itemType is one registered item of an Experience, shared by every Item of the type.
type itemType struct {
	// exp is the id of the Experience, id the item's id and name its display name.
	exp, id, name string
	maxCount      int
	icon          image.Image
}

// Item is an item of an Experience. Its data, if any, is a value of the stack that holds it.
type Item struct{ t *itemType }

// The Dragonfly interfaces that Item implements.
var (
	_ world.CustomItem = Item{}
	_ item.MaxCounter  = Item{}
)

// EncodeItem returns the item's id.
func (i Item) EncodeItem() (string, int16) { return i.t.id, 0 }

// Name is the item's display name.
func (i Item) Name() string { return i.t.name }

// Texture is the item's icon.
func (i Item) Texture() image.Image { return i.t.icon }

// Category puts the item in the items tab.
func (Item) Category() category.Category { return category.Items() }

// MaxCount is the most one stack of the item holds.
func (i Item) MaxCount() int { return i.t.maxCount }

// newItemType checks def, an item of the Experience exp beside its blocks, as the runtime does,
// and decodes its icon through cache.
func newItemType(exp string, def ItemDef, blocks []BlockDef, cache *assetCache) (*itemType, error) {
	fail := func(err error) (*itemType, error) {
		return nil, fmt.Errorf("experience %q: item %q: %w", exp, def.ID, err)
	}
	name, ok := strings.CutPrefix(def.ID, exp+":")
	if !ok || name == "" {
		return fail(fmt.Errorf("it is outside namespace %q", exp+":"))
	}
	if slices.ContainsFunc(blocks, func(b BlockDef) bool { return b.ID == def.ID }) {
		return fail(errors.New("it is also a block, whose item it would replace"))
	}
	if def.DisplayName == "" || strings.ContainsFunc(def.DisplayName, unicode.IsControl) {
		return fail(fmt.Errorf("display name %q is empty or has a control character", def.DisplayName))
	}
	if def.MaxStack < 1 || def.MaxStack > maxStackSize {
		return fail(fmt.Errorf("it stacks to %d; it needs 1 to %d", def.MaxStack, maxStackSize))
	}
	icon, err := cache.image(def.Icon)
	if err != nil {
		return fail(fmt.Errorf("icon: %w", err))
	}
	return &itemType{exp: exp, id: def.ID, name: def.DisplayName, maxCount: int(def.MaxStack), icon: icon}, nil
}

// LookupItem returns the registered item with the id.
func (r *Registry) LookupItem(id string) (Item, bool) {
	t, ok := r.items[id]
	return Item{t}, ok
}

// serverItems lists vanilla item variants and their stack limits, sorted by id and metadata.
func serverItems() []ServerItem {
	out := make([]ServerItem, 0)
	for _, it := range world.Items() {
		id, metadata := it.EncodeItem()
		if !strings.HasPrefix(id, vanillaNamespace+":") {
			continue
		}
		stack := item.NewStack(it, 1)
		if stack.Empty() {
			continue
		}
		most := min(stack.MaxCount(), maxStackSize)
		out = append(out, ServerItem{ID: id, Metadata: uint16(metadata), MaxCount: uint8(most), OffHand: canOffHand(stack), Plain: stackOf("", stack).Plain})
	}
	slices.SortFunc(out, func(a, b ServerItem) int {
		if order := strings.Compare(a.ID, b.ID); order != 0 {
			return order
		}
		return cmp.Compare(a.Metadata, b.Metadata)
	})
	return out[:min(len(out), maxServerItems)]
}

// canOffHand matches the server's offhand validator, including empty stacks used to clear it.
func canOffHand(stack item.Stack) bool {
	if stack.Empty() {
		return true
	}
	it, ok := stack.Item().(item.OffHand)
	return ok && it.OffHand()
}

// stackOf is s as the Experience exp's guest sees it: its id, metadata, count and the most one
// stack of it holds; its data only on exp's own item; and plain when it carries nothing else: no
// name, lore, enchantment, wear, anvil cost, block NBT or value but exp's data.
func stackOf(exp string, s item.Stack) ItemStack {
	id, meta := s.Item().EncodeItem()
	out := ItemStack{ID: id, Metadata: uint16(meta), Count: uint8(s.Count()), MaxCount: uint8(s.MaxCount())}
	values := s.Values()
	if own, ok := s.Item().(Item); ok && own.t.exp == exp {
		if data, ok := values[itemDataKey].([]byte); ok {
			text := hex.EncodeToString(data)
			out.Data = &text
		}
		delete(values, itemDataKey)
	}
	_, nbt := s.Item().(world.NBTer)
	out.Plain = len(values) == 0 && s.CustomName() == "" && len(s.Lore()) == 0 &&
		len(s.Enchantments()) == 0 && s.Durability() == s.MaxDurability() && s.AnvilCost() == 0 && !nbt
	return out
}

// makeStack makes the stack the Experience exp's guest staged, as the runtime checked it: one of
// exp's items, with its data, or of its blocks or the server's vanilla items, without data, each
// within the most one stack holds.
func (r *Registry) makeStack(exp string, s NewStack) (item.Stack, error) {
	var data []byte
	if s.Data != nil {
		var err error
		if data, err = hex.DecodeString(*s.Data); err != nil {
			return item.Stack{}, fmt.Errorf("%s data: %w", s.ID, err)
		}
	}
	var it world.Item
	switch own, block := r.items[s.ID], r.types[s.ID]; {
	case s.Count == 0:
		return item.Stack{}, fmt.Errorf("a stack of no %s", s.ID)
	case own != nil && own.exp == exp && s.Metadata == 0:
		if len(data) > maxItemDataBytes {
			return item.Stack{}, fmt.Errorf("%s has %d bytes of data; the limit is %d", s.ID, len(data), maxItemDataBytes)
		}
		it = Item{own}
	case block != nil && block.exp == exp && s.Metadata == 0 && data == nil:
		it = Block{t: block}
	case strings.HasPrefix(s.ID, vanillaNamespace+":") && data == nil:
		var ok bool
		if it, ok = world.ItemByName(s.ID, int16(s.Metadata)); !ok {
			return item.Stack{}, fmt.Errorf("%s:%d is no server item", s.ID, s.Metadata)
		}
		if id, metadata := it.EncodeItem(); id != s.ID || uint16(metadata) != s.Metadata {
			return item.Stack{}, fmt.Errorf("%s:%d is no server item", s.ID, s.Metadata)
		}
	default:
		return item.Stack{}, fmt.Errorf("%s:%d is not an item it may make, with data %v", s.ID, s.Metadata, data != nil)
	}
	stack := item.NewStack(it, int(s.Count))
	if stack.Empty() {
		return item.Stack{}, fmt.Errorf("%s is an empty item; clear the slot instead", s.ID)
	}
	if int(s.Count) > stack.MaxCount() {
		return item.Stack{}, fmt.Errorf("%d of %s are more than one stack, %d", s.Count, s.ID, stack.MaxCount())
	}
	if data != nil {
		stack = stack.WithValue(itemDataKey, data)
	}
	return stack, nil
}

// inventoryOf is p's inventory as the Experience exp's guest sees it, and its stacks: the main
// inventory, hotbar first, then the offhand, and the selected hotbar slot.
func inventoryOf(exp string, p *player.Player) (*Inventory, []item.Stack) {
	data := p.Data()
	slots := make([]item.Stack, inventorySlots)
	for i := range inventorySlots - 1 {
		slots[i], _ = data.Inventory.Item(i)
	}
	slots[inventorySlots-1], _ = data.OffHand.Item(0)
	inv := &Inventory{Selected: uint8(data.HeldSlot), Slots: make([]*ItemStack, inventorySlots)}
	for i, s := range slots {
		if !s.Empty() {
			stack := stackOf(exp, s)
			inv.Slots[i] = &stack
		}
	}
	return inv, slots
}

// slotHolder is the inventory that holds the actor's slot, and the slot in it.
func slotHolder(p *player.Player, slot int) (*inventory.Inventory, int) {
	data := p.Data()
	if slot == inventorySlots-1 {
		return data.OffHand, 0
	}
	return data.Inventory, slot
}
