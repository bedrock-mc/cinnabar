package localserver

import (
	"math"
	"testing"

	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl32"
	"github.com/go-gl/mathgl/mgl64"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestFlySpeedCommandParsesAndRestoresAbilities(t *testing.T) {
	withPlayer(t, world.GameModeCreative, func(tx *world.Tx, p *player.Player) {
		runCommand(t, tx, p, "speed", "3")
		previous := [2]float64{p.FlightSpeed(), p.VerticalFlightSpeed()}
		walk := p.Speed()
		h := p.Handler().(*localPlayerHandler)
		for _, args := range []string{"500", "5000", "12.5"} {
			runCommand(t, tx, p, "flyspeed", args)
			if h.flight.speed == 0 || p.FlightSpeed() != 0 || p.VerticalFlightSpeed() != 0 || p.Speed() != walk {
				t.Fatalf("/flyspeed %s: state %+v, abilities %v/%v, walking %v", args, h.flight, p.FlightSpeed(), p.VerticalFlightSpeed(), p.Speed())
			}
		}
		if h.flight.speed != 12.5 {
			t.Fatalf("decimal speed was not parsed: %v", h.flight.speed)
		}
		for _, args := range []string{"", "abc", "0", "-1", "NaN", "+Inf", "1e100", "500 extra", "off extra"} {
			runCommand(t, tx, p, "flyspeed", args)
			if h.flight.speed != 12.5 {
				t.Fatalf("invalid /flyspeed %q changed speed to %v", args, h.flight.speed)
			}
		}
		p.StartFlying()
		p.SetVelocity(mgl64.Vec3{10, 1, 0})
		runCommand(t, tx, p, "flyspeed", "off")
		if h.flight.speed != 0 || p.FlightSpeed() != previous[0] || p.VerticalFlightSpeed() != previous[1] || p.Velocity() != (mgl64.Vec3{}) {
			t.Fatalf("off did not restore flight: state %+v, abilities %v/%v, velocity %v", h.flight, p.FlightSpeed(), p.VerticalFlightSpeed(), p.Velocity())
		}
		runCommand(t, tx, p, "flyspeed", "off")
		if p.FlightSpeed() != previous[0] || p.VerticalFlightSpeed() != previous[1] {
			t.Fatal("repeated off changed restored abilities")
		}
		runCommand(t, tx, p, "flyspeed", "500")
		runCommand(t, tx, p, "speed", "2")
		if h.flight.speed != 0 || p.FlightSpeed() != protocol.AbilityBaseFlySpeed*2 || p.VerticalFlightSpeed() != protocol.AbilityBaseVerticalFlySpeed*2 {
			t.Fatal("/speed did not switch back to ability flight")
		}
	})
}

func TestFlySpeedValueDoesNotClampHighSpeeds(t *testing.T) {
	for _, speed := range []float64{0.1, 500, 5000, 1e12} {
		if got, err := flySpeedValue(speed); err != nil || got != speed {
			t.Fatalf("speed %v became %v: %v", speed, got, err)
		}
	}
	for _, speed := range []float64{0, -1, math.NaN(), math.Inf(1), math.Inf(-1), math.MaxFloat64, math.SmallestNonzeroFloat64} {
		if _, err := flySpeedValue(speed); err == nil {
			t.Fatalf("accepted invalid speed %v", speed)
		}
	}
}

func TestFlySpeedVelocity(t *testing.T) {
	for _, speed := range []float64{10, 500, 5000} {
		for _, test := range []struct {
			name  string
			move  mgl32.Vec2
			yaw   float32
			flags []int
			want  mgl64.Vec3
		}{
			{name: "forward", move: mgl32.Vec2{0, 1}, want: mgl64.Vec3{0, 0, 1}},
			{name: "east", move: mgl32.Vec2{0, 1}, yaw: -90, want: mgl64.Vec3{1, 0, 0}},
			{name: "west", move: mgl32.Vec2{0, 1}, yaw: 90, want: mgl64.Vec3{-1, 0, 0}},
			{name: "strafe", move: mgl32.Vec2{1, 0}, yaw: 90, want: mgl64.Vec3{0, 0, 1}},
			{name: "backward", move: mgl32.Vec2{0, -1}, yaw: 180, want: mgl64.Vec3{0, 0, 1}},
			{name: "analog", move: mgl32.Vec2{0.25, 0.5}, want: mgl64.Vec3{0.25, 0, 0.5}},
			{name: "diagonal", move: mgl32.Vec2{1, 1}, want: mgl64.Vec3{math.Sqrt(0.5), 0, math.Sqrt(0.5)}},
			{name: "up", flags: []int{packet.InputFlagJumping}, want: mgl64.Vec3{0, 1, 0}},
			{name: "down", flags: []int{packet.InputFlagSneaking}, want: mgl64.Vec3{0, -1, 0}},
			{name: "raw up", flags: []int{packet.InputFlagJumpCurrentRaw}, want: mgl64.Vec3{0, 1, 0}},
			{name: "raw down", flags: []int{packet.InputFlagSneakCurrentRaw}, want: mgl64.Vec3{0, -1, 0}},
			{name: "flight up", flags: []int{packet.InputFlagWantUp}, want: mgl64.Vec3{0, 1, 0}},
			{name: "flight down", flags: []int{packet.InputFlagWantDown}, want: mgl64.Vec3{0, -1, 0}},
			{name: "both", flags: []int{packet.InputFlagJumping, packet.InputFlagSneaking}},
			{name: "forward up", move: mgl32.Vec2{0, 1}, flags: []int{packet.InputFlagJumping}, want: mgl64.Vec3{0, 1, 1}},
			{name: "released"},
		} {
			t.Run(test.name, func(t *testing.T) {
				input := flightInput(test.move, test.yaw, test.flags...)
				got, ok := flySpeedVelocity(input, speed)
				want := test.want.Mul(speed / flightTicksPerSecond)
				if !ok || got.Sub(want).Len() > 1e-9 {
					t.Fatalf("speed %v: got %v, %v; want %v", speed, got, ok, want)
				}
			})
		}
	}
}

func TestFlySpeedRejectsNonFiniteInput(t *testing.T) {
	for _, input := range []*packet.PlayerAuthInput{
		flightInput(mgl32.Vec2{float32(math.NaN()), 0}, 0),
		flightInput(mgl32.Vec2{0, float32(math.Inf(1))}, 0),
		flightInput(mgl32.Vec2{0, 1}, float32(math.NaN())),
	} {
		if _, ok := flySpeedVelocity(input, 500); ok {
			t.Fatal("non-finite input produced motion")
		}
	}
}

func TestFlySpeedOnlyMovesFlyingPlayersAndAcceptsLargePositions(t *testing.T) {
	withPlayer(t, world.GameModeSurvival, func(tx *world.Tx, p *player.Player) {
		runCommand(t, tx, p, "flyspeed", "500")
		h := p.Handler().(*localPlayerHandler)
		input := flightInput(mgl32.Vec2{0, 1}, 0)
		h.flight.tick(p, input)
		if p.Velocity() != (mgl64.Vec3{}) || p.Flying() {
			t.Fatal("direct flight moved a grounded player or granted flight")
		}
		runCommand(t, tx, p, "fly", "")
		p.StartFlying()
		h.flight.tick(p, input)
		want, _ := flySpeedVelocity(input, h.flight.speed)
		if p.Velocity() != want {
			t.Fatalf("flight motion = %v; want %v", p.Velocity(), want)
		}
		position := p.Position().Add(want)
		p.Move(want, 0, 0)
		if p.Position() != position {
			t.Fatalf("large flight delta was corrected: got %v; want %v", p.Position(), position)
		}
		p.StopFlying()
		p.SetVelocity(mgl64.Vec3{})
		h.flight.tick(p, input)
		if p.Velocity() != (mgl64.Vec3{}) {
			t.Fatal("motion continued after stopping flight")
		}
	})
}

// flightInput supplies raw controls and held flags without position or mode transitions.
func flightInput(move mgl32.Vec2, yaw float32, flags ...int) *packet.PlayerAuthInput {
	input := &packet.PlayerAuthInput{RawMoveVector: move, Yaw: yaw, InputData: protocol.NewInputFlags(packet.InputFlagCount)}
	for _, flag := range flags {
		input.InputData.Set(flag)
	}
	return input
}
