// Command bedsimtrace emits canonical pinned-bedsim JSONL fixtures consumed by
// crates/sim. Standard output contains JSONL only.
package main

import (
	"crypto/sha256"
	"encoding/binary"
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

// scenarioSemantics is the authoritative PREG-to-bedsim environment query. A
// scenario world reports one homogeneous set of movement facts for every block,
// exactly as crates/sim's ScenarioWorld::block_physics does, so the semantics
// resolve from those facts rather than from a Dragonfly block type. This is the
// bedsim v0.1.3 BlockSemanticsProvider contract (simulator.go), which is what
// lets climbable, friction, and named-surface strata be observed instead of
// asserted.
type scenarioSemantics struct{ facts blockPhysics }

func (s scenarioSemantics) BlockName(dfworld.Block) string {
	// bedsim keys soul sand, slime, and bed behaviour off these exact vanilla
	// identifiers. Anything else must not collide with a name bedsim reacts to.
	switch s.facts.SurfaceResponse {
	case "soul_sand":
		return "minecraft:soul_sand"
	case "slime":
		return "minecraft:slime"
	case "bed":
		return "minecraft:bed"
	default:
		return "minecraft:stone"
	}
}

func (s scenarioSemantics) BlockFriction(dfworld.Block) float64 { return s.facts.Friction }

func (s scenarioSemantics) BlockClimbable(dfworld.Block) bool {
	return s.facts.Flags&flagClimbable != 0
}

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
	Tick       uint64     `json:"tick"`
	Position   vec3       `json:"position"`
	Velocity   vec3       `json:"velocity"`
	Movement   vec3       `json:"movement"`
	OnGround   bool       `json:"on_ground"`
	JumpDelay  uint8      `json:"jump_delay"`
	Collisions collisions `json:"collisions"`
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
	Name     string       `json:"name"`
	Origin   [3]int32     `json:"origin"`
	Revision uint64       `json:"revision"`
	Boxes    []aabb       `json:"boxes"`
	Physics  blockPhysics `json:"physics"`
	Unloaded bool         `json:"unloaded"`
}

type scenarioEvidence struct {
	Status string `json:"status"`
	Reason string `json:"reason,omitempty"`
}

type scenarioStep struct {
	World    scenarioWorld `json:"world"`
	Input    movementInput `json:"input"`
	Expected *tickResult   `json:"expected,omitempty"`
}

type scenarioScript struct {
	Scenario string           `json:"scenario"`
	Evidence scenarioEvidence `json:"evidence"`
	Initial  playerState      `json:"initial"`
	Steps    []scenarioStep   `json:"steps"`
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
	for _, scenario := range terrainScripts() {
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
	return bedsim.Simulator{
		World:          scriptedWorld{boxes: world.Boxes},
		BlockSemantics: scenarioSemantics{facts: world.Physics},
		Options:        bedsim.SimulationOptions{SprintTiming: bedsim.SprintTimingModern, IgnoreClientStepTiebreaker: true},
	}
}

func toBedsimState(state playerState) bedsim.MovementState {
	result := initialState()
	result.Pos = mgl64.Vec3{state.Position.X, state.Position.Y, state.Position.Z}
	result.Vel = mgl64.Vec3{state.Velocity.X, state.Velocity.Y, state.Velocity.Z}
	result.OnGround = state.OnGround
	// Bedrock reads the previous tick's axis collisions during the current one,
	// so a scenario that starts mid-contact must seed them.
	result.CollideX = state.Collisions.X
	result.CollideY = state.Collisions.Y
	result.CollideZ = state.Collisions.Z
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
	scripts := terrainScripts()
	names := make([]string, len(scripts))
	for index, script := range scripts {
		names[index] = script.Scenario
	}
	return names
}

func terrainScriptManifest() []scenarioScript {
	scripts := terrainScripts()
	for scriptIndex := range scripts {
		for stepIndex := range scripts[scriptIndex].Steps {
			scripts[scriptIndex].Steps[stepIndex].Expected = nil
		}
	}
	return scripts
}

const (
	flagClimbable   = uint8(1 << 0)
	flagWater       = uint8(1 << 1)
	flagLava        = uint8(1 << 2)
	flagCobweb      = uint8(1 << 3)
	flagScaffolding = uint8(1 << 5)
)

func terrainScripts() []scenarioScript {
	floorBoxes := []aabb{{Min: vec3{-16, 0, -16}, Max: vec3{16, 1, 16}}}
	ledgeBoxes := []aabb{{Min: vec3{-0.5, 0, -0.5}, Max: vec3{0.5, 1, 0.5}}}
	ordinary := blockPhysics{Friction: 0.6, HorizontalSpeedFactor: 1, VerticalSpeedFactor: 1, SurfaceResponse: "none"}
	world := func(name string, boxes []aabb, facts blockPhysics, revision uint64) scenarioWorld {
		if boxes == nil {
			boxes = []aabb{}
		}
		return scenarioWorld{Name: name, Origin: [3]int32{0, 0, 0}, Revision: revision, Boxes: boxes, Physics: facts}
	}
	grounded := playerState{Position: vec3{0, 1, 0}, OnGround: true}
	scripts := []scenarioScript{
		observedScript("flat_walk", world("flat_walk_world", floorBoxes, ordinary, 1), grounded, []movementInput{{Forward: 1}, {Forward: 1}}),
		observedScript("diagonal", world("diagonal_world", floorBoxes, ordinary, 2), grounded, []movementInput{{Forward: 1, Strafe: 1}, {Forward: 1, Strafe: 1}}),
		observedScript("sprint_jump", world("sprint_jump_world", floorBoxes, ordinary, 3), grounded, []movementInput{{Forward: 1, Jumping: true, JumpPressed: true, Sprinting: true}, {Forward: 1, Sprinting: true}}),
	}
	for index, edge := range []struct {
		name     string
		velocity vec3
	}{
		{"sneak_north", vec3{0, 0, 0.8}},
		{"sneak_south", vec3{0, 0, -0.8}},
		{"sneak_east", vec3{0.8, 0, 0}},
		{"sneak_west", vec3{-0.8, 0, 0}},
	} {
		state := grounded
		state.Velocity = edge.velocity
		scripts = append(scripts, observedScript(edge.name, world(edge.name+"_world", ledgeBoxes, ordinary, uint64(4+index)), state, []movementInput{{Sneaking: true}, {Sneaking: true}}))
	}
	headBoxes := append(append([]aabb{}, floorBoxes...), aabb{Min: vec3{-1, 3, -1}, Max: vec3{1, 3.2, 1}})
	headState := grounded
	headState.Position = vec3{0, 1, -0.5}
	headState.Velocity.Y = 0.8
	scripts = append(scripts, observedScript("head_collision", world("head_collision_world", headBoxes, ordinary, 8), headState, []movementInput{{}, {}}))

	unsupported := func(name, reason string, initial playerState, worlds []scenarioWorld, inputs []movementInput) {
		steps := make([]scenarioStep, len(worlds))
		for index := range worlds {
			steps[index] = scenarioStep{World: worlds[index], Input: inputs[index]}
		}
		scripts = append(scripts, scenarioScript{
			Scenario: name,
			Evidence: scenarioEvidence{Status: "unsupported_non_conformance", Reason: reason},
			Initial:  initial,
			Steps:    steps,
		})
	}
	stepState := grounded
	stepState.Position = vec3{0, 1, 0.4}
	stepState.Velocity.Z = 0.5
	slab := append(append([]aabb{}, floorBoxes...), aabb{Min: vec3{-0.5, 1, 0.7}, Max: vec3{0.5, 1.5, 1.7}})
	stair := append(append([]aabb{}, slab...), aabb{Min: vec3{-0.2, 1.5, 1.1}, Max: vec3{0.2, 2, 1.5}})
	unsupported("slab_step", "bedsim v0.1.3 loses grounded state after the deliberate Phase 3 step correction", stepState, []scenarioWorld{world("slab_step_0", slab, ordinary, 9), world("slab_step_1", slab, ordinary, 9)}, []movementInput{{}, {}})
	unsupported("stair_step", "bedsim v0.1.3 loses grounded state after the deliberate Phase 3 step correction", stepState, []scenarioWorld{world("stair_step_0", stair, ordinary, 10), world("stair_step_1", stair, ordinary, 10)}, []movementInput{{}, {}})

	airborne := playerState{Position: vec3{0.5, 1, 0.5}}
	climb := ordinary
	climb.Flags = flagClimbable
	descend := airborne
	descend.Velocity.Y = -1
	scripts = append(scripts,
		observedScript("ladder_ascend", world("ladder_ascend_world", nil, climb, 11), airborne, []movementInput{{Jumping: true}, {Jumping: true}}),
		observedScript("ladder_descend", world("ladder_descend_world", nil, climb, 12), descend, []movementInput{{}, {}}),
		observedScript("ladder_hold", world("ladder_hold_world", nil, climb, 13), descend, []movementInput{{Sneaking: true}, {Sneaking: true}}),
	)
	// Walking into a ladder climbs it: bedsim ascends on the *previous* tick's
	// horizontal collision as well as on a held jump. Tick 2 has no retained
	// collision, so one script witnesses both the firing and the non-firing arm.
	wallClimb := descend
	wallClimb.Collisions = collisions{Z: true}
	scripts = append(scripts, observedScript("ladder_wall_climb", world("ladder_wall_climb_world", nil, climb, 31), wallClimb, []movementInput{{}, {}}))

	water := ordinary
	water.Flags, water.FluidHeightBlocks = flagWater, 1
	fluidState := playerState{Position: vec3{0.5, 0.1, 0.5}, Velocity: vec3{0.4, -0.3, 0.2}}
	unsupported("water_enter", "bedsim v0.1.3 exposes no fluid environment oracle", fluidState, []scenarioWorld{world("water_enter_air", nil, ordinary, 14), world("water_enter_water", nil, water, 15)}, []movementInput{{}, {}})
	unsupported("water_swim", "bedsim v0.1.3 exposes no fluid environment oracle", fluidState, []scenarioWorld{world("water_swim_0", nil, water, 16), world("water_swim_1", nil, water, 16), world("water_swim_2", nil, water, 16)}, []movementInput{{Jumping: true}, {Forward: 1, Jumping: true}, {Forward: 1}})
	unsupported("water_exit", "bedsim v0.1.3 exposes no fluid environment oracle", fluidState, []scenarioWorld{world("water_exit_water", nil, water, 17), world("water_exit_air", nil, ordinary, 18)}, []movementInput{{Jumping: true}, {}})
	lava := water
	lava.Flags = flagLava
	unsupported("lava", "bedsim v0.1.3 exposes no fluid environment oracle", fluidState, []scenarioWorld{world("lava_0", nil, lava, 19), world("lava_1", nil, lava, 19)}, []movementInput{{Jumping: true}, {Forward: 1}})
	cobweb := ordinary
	cobweb.Flags = flagCobweb
	cobwebState := airborne
	cobwebState.Velocity = vec3{0.8, -0.8, 0.8}
	unsupported("cobweb", "bedsim v0.1.3 detects cobweb through BlockCollisions geometry, which a passable scenario block cannot supply without also blocking movement", cobwebState, []scenarioWorld{world("cobweb_0", nil, cobweb, 20), world("cobweb_1", nil, cobweb, 20)}, []movementInput{{}, {}})

	bounce := playerState{Position: vec3{0, 1.2, 0}, Velocity: vec3{0, -0.7, 0}}
	for revision, surface := range []struct {
		name, response string
		input          movementInput
	}{
		{"slime_bounce", "slime", movementInput{}},
		{"slime_sneak", "slime", movementInput{Sneaking: true}},
		{"bed_bounce", "bed", movementInput{}},
		{"bed_sneak", "bed", movementInput{Sneaking: true}},
	} {
		facts := ordinary
		facts.SurfaceResponse = surface.response
		scripts = append(scripts, observedScript(surface.name, world(surface.name+"_world", floorBoxes, facts, uint64(21+revision)), bounce, []movementInput{surface.input, surface.input}))
	}
	// Standing on slime damps horizontal velocity on flat ticks. The bounce
	// scripts above never produce one, so this witnesses walkOnBlock directly.
	slimeWalk := grounded
	slimeWalk.Velocity = vec3{0.3, 0, 0.3}
	slimeWalkFacts := ordinary
	slimeWalkFacts.SurfaceResponse = "slime"
	scripts = append(scripts, observedScript("slime_walk", world("slime_walk_world", floorBoxes, slimeWalkFacts, 32), slimeWalk, []movementInput{{}, {}}))

	// bedsim scales soul sand by a hardcoded 0.543 keyed on the block name,
	// while crates/sim consumes the PREG horizontal speed factor. Pinning the
	// scenario factor to bedsim's constant makes the two directly comparable.
	soulSand := ordinary
	soulSand.HorizontalSpeedFactor, soulSand.SurfaceResponse = 0.543, "soul_sand"
	scripts = append(scripts, observedScript("soul_sand", world("soul_sand_world", floorBoxes, soulSand, 24), grounded, []movementInput{{Forward: 1}, {Forward: 1}}))
	honey := ordinary
	honey.HorizontalSpeedFactor, honey.SurfaceResponse = 0.4, "honey"
	unsupported("honey", "bedsim v0.1.3 implements no honey movement stratum", grounded, []scenarioWorld{world("honey_0", floorBoxes, honey, 25), world("honey_1", floorBoxes, honey, 25)}, []movementInput{{Forward: 1}, {Forward: 1}})
	scaffolding := ordinary
	scaffolding.Flags = flagScaffolding
	unsupported("scaffolding", "bedsim v0.1.3 implements no scaffolding movement stratum", airborne, []scenarioWorld{world("scaffolding_0", nil, scaffolding, 26), world("scaffolding_1", nil, scaffolding, 26)}, []movementInput{{Jumping: true}, {}})
	for revision, bubble := range []struct{ name, response string }{{"bubble_up", "bubble_up"}, {"bubble_down", "bubble_down"}} {
		facts := water
		facts.SurfaceResponse = bubble.response
		unsupported(bubble.name, "bedsim v0.1.3 exposes no bubble-column environment oracle", playerState{Position: vec3{0.5, 0.1, 0.5}}, []scenarioWorld{world(bubble.name+"_0", nil, facts, uint64(27+revision)), world(bubble.name+"_1", nil, facts, uint64(27+revision))}, []movementInput{{}, {}})
	}
	unloaded := world("unloaded_boundary_unloaded", floorBoxes, ordinary, 30)
	unloaded.Unloaded = true
	unsupported("unloaded_boundary", "bedsim world API reports load state but the Rust error contract is not a bedsim TickResult", grounded, []scenarioWorld{world("unloaded_boundary_loaded", floorBoxes, ordinary, 29), unloaded}, []movementInput{{Forward: 1}, {Forward: 1}})
	return scripts
}

func observedScript(name string, world scenarioWorld, initial playerState, inputs []movementInput) scenarioScript {
	state := toBedsimState(initial)
	simulator := newScenarioSimulator(world)
	steps := make([]scenarioStep, 0, len(inputs))
	for index, input := range inputs {
		before := state
		result := simulator.Simulate(&state, toBedsimInput(before, input))
		expected := tickResult{
			Tick:          uint64(index) + initial.Tick + 1,
			Position:      fromVec3(result.Position),
			Velocity:      fromVec3(result.Velocity),
			Movement:      fromVec3(result.Movement),
			Collisions:    collisions{X: result.CollideX, Y: result.CollideY, Z: result.CollideZ},
			OnGround:      result.OnGround,
			Environment:   environment(world.Physics),
			WorldIdentity: identity(world),
		}
		steps = append(steps, scenarioStep{World: world, Input: input, Expected: &expected})
	}
	return scenarioScript{Scenario: name, Evidence: scenarioEvidence{Status: "bedsim_observed_with_manifest_context"}, Initial: initial, Steps: steps}
}

func environment(facts blockPhysics) movementEnvironment {
	return movementEnvironment{OnClimbable: facts.Flags&flagClimbable != 0, InWater: facts.Flags&flagWater != 0, InLava: facts.Flags&flagLava != 0, InCobweb: facts.Flags&flagCobweb != 0, InScaffolding: facts.Flags&flagScaffolding != 0, HorizontalSpeedFactor: facts.HorizontalSpeedFactor, VerticalSpeedFactor: facts.VerticalSpeedFactor, SurfaceResponse: facts.SurfaceResponse}
}

func identity(world scenarioWorld) worldIdentity {
	hash := sha256.New()
	hash.Write([]byte("sim-scenario-world-v1\x00"))
	var scratch [8]byte
	for _, coordinate := range world.Origin {
		binary.LittleEndian.PutUint32(scratch[:4], uint32(coordinate))
		hash.Write(scratch[:4])
	}
	binary.LittleEndian.PutUint64(scratch[:], world.Revision)
	hash.Write(scratch[:])
	binary.LittleEndian.PutUint32(scratch[:4], uint32(len(world.Boxes)))
	hash.Write(scratch[:4])
	for _, box := range world.Boxes {
		for _, value := range []float64{box.Min.X, box.Min.Y, box.Min.Z, box.Max.X, box.Max.Y, box.Max.Z} {
			binary.LittleEndian.PutUint64(scratch[:], math.Float64bits(value))
			hash.Write(scratch[:])
		}
	}
	for _, value := range []float64{world.Physics.Friction, world.Physics.HorizontalSpeedFactor, world.Physics.VerticalSpeedFactor, world.Physics.FluidHeightBlocks} {
		binary.LittleEndian.PutUint64(scratch[:], math.Float64bits(value))
		hash.Write(scratch[:])
	}
	hash.Write([]byte{world.Physics.Flags, surfaceCode(world.Physics.SurfaceResponse)})
	if world.Unloaded {
		hash.Write([]byte{1})
	} else {
		hash.Write([]byte{0})
	}
	var digest [32]uint8
	copy(digest[:], hash.Sum(nil))
	return worldIdentity{Protocol: 1001, IDSpace: "sequential", PregSHA256: digest, Chunks: []identityChunk{{Dimension: 0, X: world.Origin[0] >> 4, Z: world.Origin[2] >> 4, Revision: world.Revision}}}
}

func surfaceCode(response string) uint8 {
	switch response {
	case "none":
		return 0
	case "slime":
		return 1
	case "bed":
		return 2
	case "honey":
		return 3
	case "soul_sand":
		return 4
	case "bubble_up":
		return 5
	case "bubble_down":
		return 6
	default:
		panic("unbounded surface response")
	}
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
