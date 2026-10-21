package localserver

import (
	"errors"
	"math"
	"time"

	"github.com/df-mc/dragonfly/server/cmd"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
)

const (
	minSpeedMultiplier = 0.1
	maxSpeedMultiplier = 100
	// Dragonfly's sprint scales the movement attribute by this on start and divides it back on stop.
	sprintSpeedFactor = 1.3
	// Steady vanilla speeds per unit of FlySpeed and movement attribute, from the client's flight
	// and walking physics: 0.05 flies 10.9 blocks/s, 0.1 walks 4.32 blocks/s.
	flyBlocksPerSecondPerUnit  = 217.8
	walkBlocksPerSecondPerUnit = 43.17
)

// registerChatCommands registers the testing commands; every player may run them.
func registerChatCommands() {
	cmd.Register(cmd.New("speed", "Scales flight and walking speed from the vanilla defaults", nil, speedReset{}, speedSet{}))
	cmd.Register(cmd.New("flyspeed", "Sets direct flight speed in blocks/s, or turns it off", nil, flySpeedOff{}, flySpeedSet{}))
	cmd.Register(cmd.New("fly", "Toggles being allowed to fly", nil, flyToggle{}))
	cmd.Register(cmd.New("tp", "Teleports to a position", []string{"teleport"}, teleport{}))
	cmd.Register(cmd.New("give", "Adds a vanilla item to your inventory", nil, giveItem{}))
	cmd.Register(cmd.New("fixture_totem", "Applies server damage while holding a protective totem", nil, totemFixture{}))
	cmd.Register(cmd.New("weather", "Sets the weather: clear, rain or thunder", nil, weatherSet{}))
}

// weatherKind is the /weather argument.
type weatherKind string

func (weatherKind) Type() string { return "WeatherType" }

func (weatherKind) Options(cmd.Source) []string { return []string{"clear", "rain", "thunder"} }

// weatherSet holds the requested weather until the next /weather, so captures stay deterministic.
type weatherSet struct {
	Kind weatherKind `cmd:"type"`
}

func (w weatherSet) Run(_ cmd.Source, o *cmd.Output, tx *world.Tx) {
	level := tx.World()
	// Dragonfly only clears thunder while it rains, so rain first to drop any latent thunder flag.
	level.StartRaining(time.Hour)
	if w.Kind != "thunder" {
		level.StopThundering()
	}
	switch w.Kind {
	case "clear":
		level.StopRaining()
	case "thunder":
		level.StartThundering(time.Hour)
	}
	level.StopWeatherCycle()
	o.Printf("Weather set to %s until the next /weather", w.Kind)
}

// speedMultiplier clamps m to the accepted range.
func speedMultiplier(m float64) (float64, error) {
	if math.IsNaN(m) {
		return 0, errors.New("speed multiplier must be a number")
	}
	return min(max(m, minSpeedMultiplier), maxSpeedMultiplier), nil
}

// applySpeed sends the abilities fly speeds and the movement attribute scaled by m.
func applySpeed(p *player.Player, m float64) {
	if h, ok := p.Handler().(*localPlayerHandler); ok {
		h.flight.set(p, 0)
	}
	p.SetFlightSpeed(protocol.AbilityBaseFlySpeed * m)
	p.SetVerticalFlightSpeed(protocol.AbilityBaseVerticalFlySpeed * m)
	walk := protocol.AbilityBaseWalkSpeed * m
	if p.Sprinting() {
		walk *= sprintSpeedFactor
	}
	p.SetSpeed(walk)
}

func reportSpeed(o *cmd.Output, m float64) {
	fly := protocol.AbilityBaseFlySpeed * m * flyBlocksPerSecondPerUnit
	o.Printf("Speed x%g: flying ~%.0f blocks/s (sprinting ~%.0f), walking ~%.1f blocks/s", m, fly, 2*fly, protocol.AbilityBaseWalkSpeed*m*walkBlocksPerSecondPerUnit)
}

type speedSet struct {
	Multiplier cmd.Optional[float64] `cmd:"multiplier"`
}

func (s speedSet) Run(src cmd.Source, o *cmd.Output, _ *world.Tx) {
	p, ok := src.(*player.Player)
	if !ok {
		o.Error("only players can change their speed")
		return
	}
	m, err := speedMultiplier(s.Multiplier.LoadOr(1))
	if err != nil {
		o.Error(err)
		return
	}
	applySpeed(p, m)
	reportSpeed(o, m)
}

type speedReset struct {
	Reset cmd.SubCommand `cmd:"reset"`
}

func (speedReset) Run(src cmd.Source, o *cmd.Output, tx *world.Tx) {
	speedSet{}.Run(src, o, tx)
}

// flightMode lets a game mode without flight fly. Player data cannot store it and saves survival.
type flightMode struct{ world.GameMode }

func (flightMode) AllowsFlying() bool { return true }

type flyToggle struct{}

func (flyToggle) Run(src cmd.Source, o *cmd.Output, _ *world.Tx) {
	p, ok := src.(*player.Player)
	if !ok {
		o.Error("only players can fly")
		return
	}
	switch mode := p.GameMode().(type) {
	case flightMode:
		p.SetGameMode(mode.GameMode)
		o.Print("Flight disabled")
	default:
		if mode.AllowsFlying() {
			o.Print("This game mode already allows flight")
			return
		}
		p.SetGameMode(flightMode{mode})
		o.Print("Flight enabled: double-tap jump to fly")
	}
}

type teleport struct {
	Destination mgl64.Vec3 `cmd:"destination"`
}

func (t teleport) Run(src cmd.Source, o *cmd.Output, _ *world.Tx) {
	p, ok := src.(*player.Player)
	if !ok {
		o.Error("only players can teleport")
		return
	}
	p.Teleport(t.Destination)
	o.Printf("Teleported to %.1f, %.1f, %.1f", t.Destination[0], t.Destination[1], t.Destination[2])
}
