package experience

import (
	"sync"
	_ "unsafe" // for go:linkname

	_ "github.com/df-mc/dragonfly/server/item/recipe"
)

// registerVanillaRecipes registers the server's vanilla recipes, as building a server does.
//
//go:linkname registerVanillaRecipes github.com/df-mc/dragonfly/server/item/recipe.registerVanilla
func registerVanillaRecipes()

// vanillaRecipes registers the vanilla recipes once per test binary.
var vanillaRecipes = sync.OnceFunc(registerVanillaRecipes)
