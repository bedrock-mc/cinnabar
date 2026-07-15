// Command bedsimtrace emits the canonical pinned-bedsim JSONL walk/jump
// fixture consumed by crates/sim. Standard output contains JSONL only.
package main

import (
	"encoding/json"
	"fmt"
	"io"
	"os"

	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/cube"
	dfworld "github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
	"github.com/oomph-ac/bedsim"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

type floorWorld struct{}

func (floorWorld) Block(pos cube.Pos) dfworld.Block {
	if pos[1] == 0 {
		return block.Stone{}
	}
	return block.Air{}
}

func (floorWorld) BlockCollisions(pos cube.Pos) []cube.BBox {
	if pos[1] == 0 {
		return []cube.BBox{cube.Box(0, 0, 0, 1, 1, 1)}
	}
	return nil
}

func (floorWorld) GetNearbyBBoxes(query cube.BBox) []cube.BBox {
	floor := cube.Box(-16, 0, -16, 16, 1, 16)
	if floor.IntersectsWith(query) {
		return []cube.BBox{floor}
	}
	return nil
}

func (floorWorld) IsChunkLoaded(_, _ int32) bool { return true }

type vec3 struct {
	X float64 `json:"x"`
	Y float64 `json:"y"`
	Z float64 `json:"z"`
}

type movementInput struct {
	Strafe      float64 `json:"strafe"`
	Forward     float64 `json:"forward"`
	YawDegrees  float64 `json:"yaw_degrees"`
	Jumping     bool    `json:"jumping"`
	JumpPressed bool    `json:"jump_pressed"`
	Sprinting   bool    `json:"sprinting"`
	Sneaking    bool    `json:"sneaking"`
}

type collisions struct {
	X bool `json:"x"`
	Y bool `json:"y"`
	Z bool `json:"z"`
}

type tickResult struct {
	Tick       uint64     `json:"tick"`
	Position   vec3       `json:"position"`
	Velocity   vec3       `json:"velocity"`
	Movement   vec3       `json:"movement"`
	Collisions collisions `json:"collisions"`
	OnGround   bool       `json:"on_ground"`
}

type traceRecord struct {
	Input    movementInput `json:"input"`
	Expected tickResult    `json:"expected"`
}

func main() {
	if err := writeTrace(os.Stdout); err != nil {
		fmt.Fprintf(os.Stderr, "encode trace: %v\n", err)
		os.Exit(1)
	}
}

func writeTrace(output io.Writer) error {
	state := bedsim.MovementState{
		Pos:                     mgl64.Vec3{0, 1, 0},
		Size:                    mgl64.Vec3{0.6, 1.8, 1},
		Gravity:                 bedsim.NormalGravity,
		JumpHeight:              bedsim.DefaultJumpHeight,
		MovementSpeed:           0.1,
		DefaultMovementSpeed:    0.1,
		AirSpeed:                0.02,
		TicksSinceKnockback:     1,
		TicksSinceTeleport:      1,
		OnGround:                true,
		HasGravity:              true,
		Ready:                   true,
		Alive:                   true,
		GameMode:                packet.GameTypeSurvival,
		TeleportCompletionTicks: 0,
	}
	simulator := bedsim.Simulator{
		World: floorWorld{},
		Options: bedsim.SimulationOptions{
			SprintTiming:               bedsim.SprintTimingModern,
			IgnoreClientStepTiebreaker: true,
		},
	}
	script := []movementInput{
		{Forward: 1},
		{Forward: 1},
		{Forward: 1, Jumping: true, JumpPressed: true, Sprinting: true},
		{Forward: 1, Jumping: true, Sprinting: true},
		{Forward: 1},
	}
	encoder := json.NewEncoder(output)
	encoder.SetEscapeHTML(false)
	for index, input := range script {
		before := state
		result := simulator.Simulate(&state, toBedsimInput(before, input))
		record := traceRecord{
			Input: input,
			Expected: tickResult{
				Tick:       uint64(index + 1),
				Position:   fromVec3(result.Position),
				Velocity:   fromVec3(result.Velocity),
				Movement:   fromVec3(result.Movement),
				Collisions: collisions{X: result.CollideX, Y: result.CollideY, Z: result.CollideZ},
				OnGround:   result.OnGround,
			},
		}
		if err := encoder.Encode(record); err != nil {
			return err
		}
	}
	return nil
}

func toBedsimInput(before bedsim.MovementState, input movementInput) bedsim.InputState {
	return bedsim.InputState{
		MoveVector:     mgl64.Vec2{input.Strafe, input.Forward},
		Yaw:            input.YawDegrees,
		HeadYaw:        input.YawDegrees,
		ClientPos:      before.Pos,
		ClientVel:      before.Vel,
		StartSprinting: input.Sprinting && !before.Sprinting,
		StopSprinting:  !input.Sprinting && before.Sprinting,
		SprintDown:     input.Sprinting,
		StartJumping:   input.JumpPressed,
		Jumping:        input.Jumping,
		Sneaking:       input.Sneaking,
		SneakDown:      input.Sneaking,
	}
}

func fromVec3(value mgl64.Vec3) vec3 {
	return vec3{X: value.X(), Y: value.Y(), Z: value.Z()}
}
