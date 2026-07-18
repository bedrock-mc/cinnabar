// Command bedsimtrace emits canonical pinned-bedsim JSONL fixtures consumed by
// crates/sim. Standard output contains JSONL only.
package main

import (
	"encoding/json"
	"fmt"
	"io"
	"math"
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

type scriptedWorld struct{ boxes []aabb }

func (w scriptedWorld) Block(pos cube.Pos) dfworld.Block {
	for _, box := range w.boxes {
		if float64(pos[0])+1 > box.Min.X && float64(pos[0]) < box.Max.X && float64(pos[1])+1 > box.Min.Y && float64(pos[1]) < box.Max.Y && float64(pos[2])+1 > box.Min.Z && float64(pos[2]) < box.Max.Z {
			return block.Stone{}
		}
	}
	return block.Air{}
}

func (w scriptedWorld) BlockCollisions(pos cube.Pos) []cube.BBox {
	boxes := make([]cube.BBox, 0, len(w.boxes))
	for _, box := range w.boxes {
		if float64(pos[0])+1 > box.Min.X && float64(pos[0]) < box.Max.X && float64(pos[1])+1 > box.Min.Y && float64(pos[1]) < box.Max.Y && float64(pos[2])+1 > box.Min.Z && float64(pos[2]) < box.Max.Z {
			boxes = append(boxes, cube.Box(box.Min.X-float64(pos[0]), box.Min.Y-float64(pos[1]), box.Min.Z-float64(pos[2]), box.Max.X-float64(pos[0]), box.Max.Y-float64(pos[1]), box.Max.Z-float64(pos[2])))
		}
	}
	return boxes
}

func (w scriptedWorld) GetNearbyBBoxes(query cube.BBox) []cube.BBox {
	boxes := make([]cube.BBox, 0, len(w.boxes))
	for _, box := range w.boxes {
		candidate := cube.Box(box.Min.X, box.Min.Y, box.Min.Z, box.Max.X, box.Max.Y, box.Max.Z)
		if candidate.IntersectsWith(query) {
			boxes = append(boxes, candidate)
		}
	}
	return boxes
}

func (scriptedWorld) IsChunkLoaded(_, _ int32) bool { return true }

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
	Tick          uint64              `json:"tick"`
	Position      vec3                `json:"position"`
	Velocity      vec3                `json:"velocity"`
	Movement      vec3                `json:"movement"`
	Collisions    collisions          `json:"collisions"`
	OnGround      bool                `json:"on_ground"`
	Environment   movementEnvironment `json:"environment"`
	WorldIdentity worldIdentity       `json:"world_identity"`
}

type playerState struct {
	Tick      uint64 `json:"tick"`
	Position  vec3   `json:"position"`
	Velocity  vec3   `json:"velocity"`
	Movement  vec3   `json:"movement"`
	OnGround  bool   `json:"on_ground"`
	JumpDelay uint8  `json:"jump_delay"`
}

type movementEnvironment struct {
	OnClimbable           bool    `json:"on_climbable"`
	InWater               bool    `json:"in_water"`
	InLava                bool    `json:"in_lava"`
	InCobweb              bool    `json:"in_cobweb"`
	InPowderSnow          bool    `json:"in_powder_snow"`
	InScaffolding         bool    `json:"in_scaffolding"`
	HorizontalSpeedFactor float64 `json:"horizontal_speed_factor"`
	VerticalSpeedFactor   float64 `json:"vertical_speed_factor"`
	SurfaceResponse       string  `json:"surface_response"`
}

type identityChunk struct {
	Dimension int32  `json:"dimension"`
	X         int32  `json:"x"`
	Z         int32  `json:"z"`
	Revision  uint64 `json:"revision"`
}

type worldIdentity struct {
	Protocol   uint32          `json:"protocol"`
	IDSpace    string          `json:"id_space"`
	PregSHA256 [32]uint8       `json:"preg_sha256"`
	Chunks     []identityChunk `json:"chunks"`
}

type aabb struct {
	Min vec3 `json:"min"`
	Max vec3 `json:"max"`
}

type blockPhysics struct {
	Friction              float64 `json:"friction"`
	HorizontalSpeedFactor float64 `json:"horizontal_speed_factor"`
	VerticalSpeedFactor   float64 `json:"vertical_speed_factor"`
	FluidHeightBlocks     float64 `json:"fluid_height_blocks"`
	Flags                 uint8   `json:"flags"`
	SurfaceResponse       string  `json:"surface_response"`
}

type scenarioWorld struct {
	Name         string       `json:"name"`
	Boxes        []aabb       `json:"boxes"`
	Physics      blockPhysics `json:"physics"`
	IdentitySeed uint8        `json:"identity_seed"`
	Unloaded     bool         `json:"unloaded"`
}

type scenarioRecord struct {
	Scenario      string        `json:"scenario"`
	World         scenarioWorld `json:"world"`
	Initial       playerState   `json:"initial"`
	Input         movementInput `json:"input"`
	Expected      *tickResult   `json:"expected,omitempty"`
	ExpectedError string        `json:"expected_error,omitempty"`
}

type traceRecord struct {
	Input    movementInput   `json:"input"`
	Expected basicTickResult `json:"expected"`
}

type basicTickResult struct {
	Tick       uint64     `json:"tick"`
	Position   vec3       `json:"position"`
	Velocity   vec3       `json:"velocity"`
	Movement   vec3       `json:"movement"`
	Collisions collisions `json:"collisions"`
	OnGround   bool       `json:"on_ground"`
}

func main() {
	write := writeTrace
	if len(os.Args) == 2 && os.Args[1] == "--terrain" {
		write = writeTerrainTrace
	} else if len(os.Args) != 1 {
		fmt.Fprintln(os.Stderr, "usage: bedsimtrace [--terrain]")
		os.Exit(2)
	}
	if err := write(os.Stdout); err != nil {
		fmt.Fprintf(os.Stderr, "encode trace: %v\n", err)
		os.Exit(1)
	}
}

func writeTrace(output io.Writer) error {
	return writeScriptTrace(output, basicScript())
}

func writeTerrainTrace(output io.Writer) error {
	encoder := json.NewEncoder(output)
	encoder.SetEscapeHTML(false)
	for _, scenario := range terrainScenarios() {
		if err := encoder.Encode(scenario); err != nil {
			return err
		}
	}
	return nil
}

func initialState() bedsim.MovementState {
	return bedsim.MovementState{
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
}

func newSimulator() bedsim.Simulator {
	return bedsim.Simulator{
		World: floorWorld{},
		Options: bedsim.SimulationOptions{
			SprintTiming:               bedsim.SprintTimingModern,
			IgnoreClientStepTiebreaker: true,
		},
	}
}

func newScenarioSimulator(world scenarioWorld) bedsim.Simulator {
	return bedsim.Simulator{World: scriptedWorld{boxes: world.Boxes}, Options: bedsim.SimulationOptions{SprintTiming: bedsim.SprintTimingModern, IgnoreClientStepTiebreaker: true}}
}

func toBedsimState(state playerState) bedsim.MovementState {
	result := initialState()
	result.Pos = mgl64.Vec3{state.Position.X, state.Position.Y, state.Position.Z}
	result.Vel = mgl64.Vec3{state.Velocity.X, state.Velocity.Y, state.Velocity.Z}
	result.OnGround = state.OnGround
	return result
}

func basicScript() []movementInput {
	return []movementInput{
		{Forward: 1},
		{Forward: 1},
		{Forward: 1, Jumping: true, JumpPressed: true, Sprinting: true},
		{Forward: 1, Jumping: true, Sprinting: true},
		{Forward: 1},
	}
}

func terrainScriptNames() []string {
	scenarios := terrainScenarios()
	names := make([]string, len(scenarios))
	for index, scenario := range scenarios {
		names[index] = scenario.Scenario
	}
	return names
}

func terrainScriptManifest() []scenarioRecord {
	scenarios := terrainScenarios()
	for index := range scenarios {
		scenarios[index].Expected = nil
	}
	return scenarios
}

const (
	flagClimbable   = uint8(1 << 0)
	flagWater       = uint8(1 << 1)
	flagLava        = uint8(1 << 2)
	flagCobweb      = uint8(1 << 3)
	flagScaffolding = uint8(1 << 5)
)

func terrainScenarios() []scenarioRecord {
	floor := []aabb{{Min: vec3{-16, 0, -16}, Max: vec3{16, 1, 16}}}
	ledge := []aabb{{Min: vec3{-0.5, 0, -0.5}, Max: vec3{0.5, 1, 0.5}}}
	physics := blockPhysics{Friction: 0.6, HorizontalSpeedFactor: 1, VerticalSpeedFactor: 1, SurfaceResponse: "none"}
	grounded := playerState{Position: vec3{0, 1, 0}, OnGround: true}
	airborne := playerState{Position: vec3{0.5, 1, 0.5}}
	makeScenario := func(name string, boxes []aabb, facts blockPhysics, initial playerState, input movementInput) scenarioRecord {
		if boxes == nil {
			boxes = []aabb{}
		}
		seed := uint8(len(name) + len(boxes)*17)
		record := scenarioRecord{Scenario: name, World: scenarioWorld{Name: name + "_world", Boxes: boxes, Physics: facts, IdentitySeed: seed}, Initial: initial, Input: input}
		result := referenceTick(record)
		if name == "slab_step" || name == "stair_step" {
			// Phase 3 deliberately corrects bedsim v0.1.3's loss of grounded
			// state after a successful step; the full-state golden pins that fix.
			result.OnGround = true
		}
		record.Expected = &result
		return record
	}
	scenarios := []scenarioRecord{
		makeScenario("flat_walk", floor, physics, grounded, movementInput{Forward: 1}),
		makeScenario("diagonal", floor, physics, grounded, movementInput{Forward: 1, Strafe: 1}),
		makeScenario("sprint_jump", floor, physics, grounded, movementInput{Forward: 1, Jumping: true, JumpPressed: true, Sprinting: true}),
	}
	slab := append(append([]aabb{}, floor...), aabb{Min: vec3{-0.5, 1, 0.7}, Max: vec3{0.5, 1.5, 1.7}})
	stepState := grounded
	stepState.Position = vec3{0, 1, 0.4}
	stepState.Velocity.Z = 0.5
	scenarios = append(scenarios,
		makeScenario("slab_step", slab, physics, stepState, movementInput{}),
		makeScenario("stair_step", append(slab, aabb{Min: vec3{-0.2, 1.5, 1.1}, Max: vec3{0.2, 2, 1.5}}), physics, stepState, movementInput{}),
	)
	for _, edge := range []struct {
		name     string
		velocity vec3
	}{
		{"sneak_north", vec3{0, 0, 0.8}}, {"sneak_south", vec3{0, 0, -0.8}},
		{"sneak_east", vec3{0.8, 0, 0}}, {"sneak_west", vec3{-0.8, 0, 0}},
	} {
		state := grounded
		state.Velocity = edge.velocity
		scenarios = append(scenarios, makeScenario(edge.name, ledge, physics, state, movementInput{Sneaking: true}))
	}
	headWorld := append(append([]aabb{}, floor...), aabb{Min: vec3{-1, 3, -1}, Max: vec3{1, 3.2, 1}})
	headState := grounded
	headState.Position = vec3{0, 1, -0.5}
	headState.Velocity.Y = 0.8
	scenarios = append(scenarios, makeScenario("head_collision", headWorld, physics, headState, movementInput{}))
	climb := physics
	climb.Flags = flagClimbable
	climbDown := airborne
	climbDown.Velocity.Y = -1
	scenarios = append(scenarios,
		makeScenario("ladder_ascend", nil, climb, airborne, movementInput{Jumping: true}),
		makeScenario("ladder_descend", nil, climb, climbDown, movementInput{}),
		makeScenario("ladder_hold", nil, climb, climbDown, movementInput{Sneaking: true}),
	)
	water := physics
	water.Flags, water.FluidHeightBlocks, water.VerticalSpeedFactor = flagWater, 1, 1
	fluidState := playerState{Position: vec3{0.5, 0.1, 0.5}, Velocity: vec3{0.4, -0.3, 0.2}}
	scenarios = append(scenarios,
		makeScenario("water_enter", nil, water, fluidState, movementInput{}),
		makeScenario("water_swim", nil, water, fluidState, movementInput{Forward: 1, Jumping: true}),
		makeScenario("water_exit", nil, physics, playerState{Position: vec3{0.5, 2, 0.5}, Velocity: vec3{0.2, 0.1, 0.1}}, movementInput{}),
	)
	lava := water
	lava.Flags = flagLava
	scenarios = append(scenarios, makeScenario("lava", nil, lava, fluidState, movementInput{Forward: 1, Jumping: true}))
	cobweb := physics
	cobweb.Flags = flagCobweb
	cobwebState := airborne
	cobwebState.Velocity = vec3{0.8, -0.8, 0.8}
	scenarios = append(scenarios, makeScenario("cobweb", nil, cobweb, cobwebState, movementInput{}))
	bounceState := playerState{Position: vec3{0, 1.2, 0}, Velocity: vec3{0, -0.7, 0}}
	for _, surface := range []struct {
		name, response string
		input          movementInput
	}{
		{"slime_bounce", "slime", movementInput{}}, {"slime_sneak", "slime", movementInput{Sneaking: true}}, {"bed_bounce", "bed", movementInput{}},
	} {
		facts := physics
		facts.SurfaceResponse = surface.response
		scenarios = append(scenarios, makeScenario(surface.name, floor, facts, bounceState, surface.input))
	}
	for _, surface := range []struct{ name, response string }{{"soul_sand", "soul_sand"}, {"honey", "honey"}} {
		facts := physics
		facts.HorizontalSpeedFactor, facts.SurfaceResponse = 0.4, surface.response
		scenarios = append(scenarios, makeScenario(surface.name, floor, facts, grounded, movementInput{Forward: 1}))
	}
	scaffolding := physics
	scaffolding.Flags = flagScaffolding
	scenarios = append(scenarios, makeScenario("scaffolding", nil, scaffolding, airborne, movementInput{Jumping: true}))
	for _, bubble := range []struct{ name, response string }{{"bubble_up", "bubble_up"}, {"bubble_down", "bubble_down"}} {
		facts := water
		facts.SurfaceResponse = bubble.response
		state := playerState{Position: vec3{0.5, 0.1, 0.5}}
		scenarios = append(scenarios, makeScenario(bubble.name, nil, facts, state, movementInput{}))
	}
	scenarios = append(scenarios, scenarioRecord{
		Scenario:      "unloaded_boundary",
		World:         scenarioWorld{Name: "unloaded_boundary_world", Boxes: []aabb{}, Physics: physics, IdentitySeed: 255, Unloaded: true},
		Initial:       grounded,
		ExpectedError: "unloaded_boundary",
	})
	return scenarios
}

func referenceTick(record scenarioRecord) tickResult {
	if record.World.Physics.Flags != 0 || record.World.Physics.SurfaceResponse != "none" || record.World.Physics.HorizontalSpeedFactor != 1 {
		return referenceEnvironmentTick(record)
	}
	state := toBedsimState(record.Initial)
	simulator := newScenarioSimulator(record.World)
	result := simulator.Simulate(&state, toBedsimInput(state, record.Input))
	return tickResult{
		Tick: record.Initial.Tick + 1, Position: fromVec3(result.Position), Velocity: fromVec3(result.Velocity), Movement: fromVec3(result.Movement),
		Collisions: collisions{X: result.CollideX, Y: result.CollideY, Z: result.CollideZ}, OnGround: result.OnGround,
		Environment: environment(record.World.Physics), WorldIdentity: identity(record.World.IdentitySeed),
	}
}

func referenceEnvironmentTick(record scenarioRecord) tickResult {
	facts, input, state := record.World.Physics, record.Input, record.Initial
	env := environment(facts)
	velocity := state.Velocity
	grounded := state.OnGround
	friction := 0.91
	if grounded {
		friction *= facts.Friction
	}
	if input.Forward != 0 || input.Strafe != 0 {
		speed := 0.02 * facts.HorizontalSpeedFactor
		if grounded {
			speed = 0.1 * facts.HorizontalSpeedFactor * (0.16277136 / (friction * friction * friction))
		} else if input.Sprinting {
			speed = 0.026 * facts.HorizontalSpeedFactor
		}
		force := speed / math.Max(1, math.Hypot(input.Strafe*0.98, input.Forward*0.98))
		velocity.X += input.Strafe * 0.98 * force
		velocity.Z += input.Forward * 0.98 * force
	}
	if env.OnClimbable || env.InScaffolding {
		velocity.Y = math.Max(velocity.Y, -0.2)
		if input.Jumping {
			velocity.Y = 0.2
		} else if input.Sneaking && velocity.Y < 0 {
			velocity.Y = 0
		}
	}
	if env.InWater || env.InLava {
		if input.Jumping {
			velocity.Y += 0.04
		}
		velocity.Y *= facts.VerticalSpeedFactor
	}
	if env.InCobweb {
		velocity.X, velocity.Y, velocity.Z = velocity.X*0.25, velocity.Y*0.05, velocity.Z*0.25
	}
	movement := velocity
	position := vec3{state.Position.X + movement.X, state.Position.Y + movement.Y, state.Position.Z + movement.Z}
	collidedY := false
	if len(record.World.Boxes) != 0 && position.Y < 1 {
		movement.Y, position.Y, collidedY = 1-state.Position.Y, 1, true
	}
	if collidedY {
		switch facts.SurfaceResponse {
		case "slime":
			if !grounded && !input.Sneaking && velocity.Y < 0 {
				velocity.Y = -velocity.Y
			} else {
				velocity.Y = 0
			}
		case "bed":
			if !grounded && velocity.Y < 0 {
				velocity.Y = math.Min(-0.66*velocity.Y, 1)
			} else {
				velocity.Y = 0
			}
		default:
			velocity.Y = 0
		}
	}
	if env.InCobweb {
		velocity = vec3{}
	} else if env.InWater || env.InLava {
		drag := 0.8
		if env.InLava {
			drag = 0.5
		}
		velocity.X, velocity.Y, velocity.Z = velocity.X*drag, (velocity.Y-0.02)*drag, velocity.Z*drag
	} else {
		velocity.X, velocity.Y, velocity.Z = velocity.X*friction, (velocity.Y-0.08)*0.98, velocity.Z*friction
	}
	if facts.SurfaceResponse == "bubble_up" {
		velocity.Y = math.Max(velocity.Y, 0.1)
	}
	if facts.SurfaceResponse == "bubble_down" {
		velocity.Y = math.Min(velocity.Y, -0.1)
	}
	return tickResult{Tick: state.Tick + 1, Position: position, Velocity: velocity, Movement: movement, Collisions: collisions{Y: collidedY}, OnGround: collidedY || (grounded && math.Abs(movement.Y) <= 1e-5), Environment: env, WorldIdentity: identity(record.World.IdentitySeed)}
}

func environment(facts blockPhysics) movementEnvironment {
	return movementEnvironment{OnClimbable: facts.Flags&flagClimbable != 0, InWater: facts.Flags&flagWater != 0, InLava: facts.Flags&flagLava != 0, InCobweb: facts.Flags&flagCobweb != 0, InScaffolding: facts.Flags&flagScaffolding != 0, HorizontalSpeedFactor: facts.HorizontalSpeedFactor, VerticalSpeedFactor: facts.VerticalSpeedFactor, SurfaceResponse: facts.SurfaceResponse}
}

func identity(seed uint8) worldIdentity {
	var hash [32]uint8
	for index := range hash {
		hash[index] = seed
	}
	return worldIdentity{Protocol: 1001, IDSpace: "sequential", PregSHA256: hash, Chunks: []identityChunk{{Dimension: 0, X: 0, Z: 0, Revision: uint64(seed)}, {Dimension: 0, X: 1, Z: 0, Revision: uint64(seed) + 1}}}
}

func writeScriptTrace(output io.Writer, script []movementInput) error {
	state := initialState()
	simulator := newSimulator()
	encoder := json.NewEncoder(output)
	encoder.SetEscapeHTML(false)
	for index, input := range script {
		before := state
		result := simulator.Simulate(&state, toBedsimInput(before, input))
		record := traceRecord{
			Input: input,
			Expected: basicTickResult{
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
