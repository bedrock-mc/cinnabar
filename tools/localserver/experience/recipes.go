package experience

import (
	"cmp"
	"slices"

	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/item/recipe"
)

// craftingTable is the block whose recipes a crafting grid takes.
const craftingTable = "crafting_table"

// serverRecipes lists the server's crafting-table recipes for load, shaped and shapeless, in the
// order the server prefers them (lowest priority first). An input item tag becomes its members,
// any metadata. A recipe the protocol cannot carry (more than one output, an output past a
// stack, an input of more items than maxRecipeInputItems) is left out, so no grid matches it.
func serverRecipes() []Recipe {
	var all []recipe.Recipe
	for _, r := range recipe.Recipes() {
		switch r.(type) {
		case recipe.Shaped, recipe.Shapeless:
			if r.Block() == craftingTable {
				all = append(all, r)
			}
		}
	}
	slices.SortStableFunc(all, func(a, b recipe.Recipe) int { return cmp.Compare(a.Priority(), b.Priority()) })
	out := make([]Recipe, 0, len(all))
	for _, r := range all {
		if converted, ok := wireRecipe(r); ok {
			out = append(out, converted)
		}
		if len(out) == maxRecipes {
			break
		}
	}
	return out
}

// wireRecipe is r in the protocol's form.
func wireRecipe(r recipe.Recipe) (Recipe, bool) {
	outputs := r.Output()
	if len(outputs) != 1 || outputs[0].Empty() || outputs[0].Count() > maxStackSize {
		return Recipe{}, false
	}
	id, meta := outputs[0].Item().EncodeItem()
	wire := Recipe{
		Inputs: make([][]ItemMatch, 0, len(r.Input())),
		Output: Crafted{ID: id, Metadata: uint16(meta), Count: uint8(outputs[0].Count())},
	}
	if shaped, ok := r.(recipe.Shaped); ok {
		shape := [2]uint8{uint8(shaped.Shape().Width()), uint8(shaped.Shape().Height())}
		wire.Shape = &shape
	}
	for _, input := range r.Input() {
		matches, ok := inputMatches(input)
		if !ok {
			return Recipe{}, false
		}
		wire.Inputs = append(wire.Inputs, matches)
	}
	return wire, true
}

// inputMatches lists the items input accepts; an empty input is an empty cell.
func inputMatches(input recipe.Item) ([]ItemMatch, bool) {
	matches := []ItemMatch{}
	switch in := input.(type) {
	case item.Stack:
		if in.Empty() {
			return matches, true
		}
		id, meta := in.Item().EncodeItem()
		match := ItemMatch{ID: id}
		// The recipe data marks an input of any metadata so.
		if anyMeta, _ := in.Value("variants"); anyMeta != true {
			metadata := uint16(meta)
			match.Metadata = &metadata
		}
		matches = append(matches, match)
	case recipe.ItemTag:
		for _, id := range recipe.ItemsInTag(in.Tag()) {
			matches = append(matches, ItemMatch{ID: id})
		}
	default:
		return nil, false
	}
	return matches, len(matches) <= maxRecipeInputItems
}
