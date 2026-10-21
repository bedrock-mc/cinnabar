package localserver

import (
	"testing"

	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
)

// TestTotemFixtureRequiresProtection verifies the fixture cannot kill an unprotected player.
func TestTotemFixtureRequiresProtection(t *testing.T) {
	withPlayer(t, world.GameModeSurvival, func(tx *world.Tx, p *player.Player) {
		health := p.Health()
		runCommand(t, tx, p, "fixture_totem", "")
		if p.Health() != health || p.Dead() {
			t.Fatal("unprotected fixture changed player health")
		}
		runCommand(t, tx, p, "give", "totem_of_undying")
		runCommand(t, tx, p, "fixture_totem", "")
		hand, _ := p.HeldItems()
		if !hand.Empty() || p.Dead() || p.Health() <= 0 {
			t.Fatalf("fixture did not consume the totem and preserve life: hand=%v health=%v", hand, p.Health())
		}
	})
}
