package main

import (
	"math"
	"testing"

	"github.com/df-mc/dragonfly/server/cmd"
	"github.com/df-mc/dragonfly/server/entity"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
)

// runCommand runs a registered chat command as p.
func runCommand(t *testing.T, tx *world.Tx, p *player.Player, name, args string) {
	t.Helper()
	c, ok := cmd.ByAlias(name)
	if !ok {
		t.Fatalf("command %q is not registered", name)
	}
	c.Execute(args, p, tx)
}

// withPlayer runs f on a player in a fresh synchronous world.
func withPlayer(t *testing.T, mode world.GameMode, f func(tx *world.Tx, p *player.Player)) {
	t.Helper()
	registerChatCommands()
	w := world.Config{Synchronous: true, Entities: entity.DefaultRegistry}.New()
	t.Cleanup(func() { _ = w.Close() })
	task := w.Do(func(tx *world.Tx) {
		handle := world.EntitySpawnOpts{Position: mgl64.Vec3{0, 10, 0}}.New(player.Type, player.Config{Name: "tester", GameMode: mode})
		f(tx, tx.AddEntity(handle).(*player.Player))
	})
	if err := task.Err(); err != nil {
		t.Fatal(err)
	}
}

func TestSpeedMultiplierClamps(t *testing.T) {
	for _, c := range []struct{ in, want float64 }{
		{12, 12}, {0, minSpeedMultiplier}, {-5, minSpeedMultiplier}, {1e9, maxSpeedMultiplier}, {math.Inf(1), maxSpeedMultiplier},
	} {
		if got, err := speedMultiplier(c.in); err != nil || got != c.want {
			t.Fatalf("speedMultiplier(%v) = %v, %v; want %v", c.in, got, err, c.want)
		}
	}
	if _, err := speedMultiplier(math.NaN()); err == nil {
		t.Fatal("NaN must be rejected")
	}
}

func TestSpeedCommandScalesAndResets(t *testing.T) {
	withPlayer(t, world.GameModeCreative, func(tx *world.Tx, p *player.Player) {
		check := func(args string, m float64) {
			t.Helper()
			runCommand(t, tx, p, "speed", args)
			if p.FlightSpeed() != protocol.AbilityBaseFlySpeed*m ||
				p.VerticalFlightSpeed() != protocol.AbilityBaseVerticalFlySpeed*m ||
				p.Speed() != protocol.AbilityBaseWalkSpeed*m {
				t.Fatalf("/speed %s: fly %v, vertical %v, walk %v; want x%v", args, p.FlightSpeed(), p.VerticalFlightSpeed(), p.Speed(), m)
			}
		}
		check("12", 12)
		check("reset", 1)
		check("1000", maxSpeedMultiplier)
		check("", 1)
		check("abc", 1) // a parse error leaves the speed alone
	})
}

// A speed set mid-sprint must return to the scaled walk speed once Dragonfly undoes the sprint.
func TestSpeedCommandKeepsSprintScaling(t *testing.T) {
	withPlayer(t, world.GameModeCreative, func(tx *world.Tx, p *player.Player) {
		p.StartSprinting()
		runCommand(t, tx, p, "speed", "4")
		p.StopSprinting()
		if want := protocol.AbilityBaseWalkSpeed * 4; math.Abs(p.Speed()-want) > 1e-9 {
			t.Fatalf("walk speed after sprint = %v, want %v", p.Speed(), want)
		}
	})
}

func TestFlyCommandTogglesFlight(t *testing.T) {
	withPlayer(t, world.GameModeAdventure, func(tx *world.Tx, p *player.Player) {
		runCommand(t, tx, p, "fly", "")
		if !p.GameMode().AllowsFlying() || p.GameMode().AllowsEditing() {
			t.Fatal("/fly must allow flight and keep the rest of adventure mode")
		}
		p.StartFlying()
		runCommand(t, tx, p, "fly", "")
		if p.GameMode() != world.GameModeAdventure || p.Flying() {
			t.Fatalf("second /fly left mode %T, flying %v", p.GameMode(), p.Flying())
		}
	})
	withPlayer(t, world.GameModeCreative, func(tx *world.Tx, p *player.Player) {
		runCommand(t, tx, p, "fly", "")
		if p.GameMode() != world.GameModeCreative {
			t.Fatalf("/fly changed creative to %T", p.GameMode())
		}
	})
}

func TestTeleportCommand(t *testing.T) {
	withPlayer(t, world.GameModeCreative, func(tx *world.Tx, p *player.Player) {
		runCommand(t, tx, p, "tp", "1000 80 -2000")
		if p.Position() != (mgl64.Vec3{1000, 80, -2000}) {
			t.Fatalf("position = %v", p.Position())
		}
	})
}
