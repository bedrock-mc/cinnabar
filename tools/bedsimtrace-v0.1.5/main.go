// Command bedsimtrace-v0.1.5 emits the canonical v0.1.5 liquid JSONL fixture.
// Standard output contains JSONL only.
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
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl32"
	"github.com/oomph-ac/bedsim"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

const flagWater = uint8(1 << 1)

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

// physicsRegion assigns one set of authoritative block movement facts to the
// half-open cuboid from Min through Max. It gives the fixture an explicit liquid
// layer without treating all empty space as water.
type physicsRegion struct {
	Min     [3]int32     `json:"min"`
	Max     [3]int32     `json:"max"`
	Physics blockPhysics `json:"physics"`
}

type scenarioWorld struct {
	Name           string          `json:"name"`
	Origin         [3]int32        `json:"origin"`
	Revision       uint64          `json:"revision"`
	Boxes          []aabb          `json:"boxes"`
	Physics        blockPhysics    `json:"physics"`
	PhysicsRegions []physicsRegion `json:"physics_regions"`
	Unloaded       bool            `json:"unloaded"`
}

type scenarioEvidence struct {
	Status string `json:"status"`
}

type scenarioStep struct {
	World    scenarioWorld `json:"world"`
	Input    movementInput `json:"input"`
	Expected *tickResult   `json:"expected"`
}

// observedStep is one generator input paired with its authoritative world.
type observedStep struct {
	world scenarioWorld
	input movementInput
}

type scenarioScript struct {
	Scenario string           `json:"scenario"`
	Evidence scenarioEvidence `json:"evidence"`
	Initial  playerState      `json:"initial"`
	Steps    []scenarioStep   `json:"steps"`
}

// scriptedWorld adapts the compact fixture world to bedsim's collision and
// liquid providers. Liquids remain passable while Boxes remain authoritative
// collision geometry.
type scriptedWorld struct {
	world scenarioWorld
}

// Block exposes solid fixture boxes as stone and all other primary-layer space as air.
func (w scriptedWorld) Block(pos cube.Pos) world.Block {
	for _, box := range w.world.Boxes {
		if float64(pos[0])+1 > box.Min.X && float64(pos[0]) < box.Max.X && float64(pos[1])+1 > box.Min.Y && float64(pos[1]) < box.Max.Y && float64(pos[2])+1 > box.Min.Z && float64(pos[2]) < box.Max.Z {
			return block.Stone{}
		}
	}
	return block.Air{}
}

// BlockCollisions returns fixture collision boxes translated into one block's local coordinates.
func (w scriptedWorld) BlockCollisions(pos cube.Pos) []cube.BBox32 {
	boxes := make([]cube.BBox32, 0, len(w.world.Boxes))
	for _, box := range w.world.Boxes {
		if float64(pos[0])+1 > box.Min.X && float64(pos[0]) < box.Max.X && float64(pos[1])+1 > box.Min.Y && float64(pos[1]) < box.Max.Y && float64(pos[2])+1 > box.Min.Z && float64(pos[2]) < box.Max.Z {
			boxes = append(boxes, cube.Box32(float32(box.Min.X-float64(pos[0])), float32(box.Min.Y-float64(pos[1])), float32(box.Min.Z-float64(pos[2])), float32(box.Max.X-float64(pos[0])), float32(box.Max.Y-float64(pos[1])), float32(box.Max.Z-float64(pos[2]))))
		}
	}
	return boxes
}

// GetNearbyBBoxes returns every fixture box intersecting a bounded collision query.
func (w scriptedWorld) GetNearbyBBoxes(query cube.BBox32) []cube.BBox32 {
	boxes := make([]cube.BBox32, 0, len(w.world.Boxes))
	for _, box := range w.world.Boxes {
		candidate := cube.Box32(float32(box.Min.X), float32(box.Min.Y), float32(box.Min.Z), float32(box.Max.X), float32(box.Max.Y), float32(box.Max.Z))
		if candidate.IntersectsWith(query) {
			boxes = append(boxes, candidate)
		}
	}
	return boxes
}

// IsChunkLoaded makes the fixture's explicit unloaded flag authoritative for every chunk.
func (w scriptedWorld) IsChunkLoaded(_, _ int32) bool { return !w.world.Unloaded }

// Liquid exposes only water facts stored in the fixture's explicit physics regions.
func (w scriptedWorld) Liquid(pos cube.Pos) (world.Liquid, bool) {
	if w.physicsAt([3]int32{int32(pos[0]), int32(pos[1]), int32(pos[2])}).Flags&flagWater != 0 {
		return block.Water{Depth: 8, Still: true}, true
	}
	return nil, false
}

// physicsAt returns the region facts for one block, or the world default.
func (w scriptedWorld) physicsAt(pos [3]int32) blockPhysics {
	for _, region := range w.world.PhysicsRegions {
		if pos[0] >= region.Min[0] && pos[0] < region.Max[0] && pos[1] >= region.Min[1] && pos[1] < region.Max[1] && pos[2] >= region.Min[2] && pos[2] < region.Max[2] {
			return region.Physics
		}
	}
	return w.world.Physics
}

func main() {
	if len(os.Args) != 1 {
		fmt.Fprintln(os.Stderr, "usage: bedsimtrace-v0.1.5")
		os.Exit(2)
	}
	if err := writeTrace(os.Stdout); err != nil {
		fmt.Fprintf(os.Stderr, "encode trace: %v\n", err)
		os.Exit(1)
	}
}

// writeTrace emits all exit-probe cases from one deterministic script set.
func writeTrace(output io.Writer) error {
	encoder := json.NewEncoder(output)
	encoder.SetEscapeHTML(false)
	for _, script := range liquidExitScripts() {
		if err := encoder.Encode(script); err != nil {
			return err
		}
	}
	return nil
}

// initialState supplies the non-liquid movement defaults required by bedsim.
func initialState() bedsim.MovementState {
	return bedsim.MovementState{
		Pos:                  mgl32.Vec3{0.5, 0.5, 0.5},
		Size:                 mgl32.Vec3{0.6, 1.8, 1},
		Gravity:              bedsim.NormalGravity,
		JumpHeight:           bedsim.DefaultJumpHeight,
		MovementSpeed:        0.1,
		DefaultMovementSpeed: 0.1,
		AirSpeed:             0.02,
		TicksSinceKnockback:  1,
		TicksSinceTeleport:   1,
		HasGravity:           true,
		Ready:                true,
		Alive:                true,
		GameMode:             packet.GameTypeSurvival,
	}
}

// toBedsimState converts the fixture state while seeding the prior collision
// flags that bedsim observes at the start of a tick.
func toBedsimState(state playerState) bedsim.MovementState {
	result := initialState()
	result.Pos = mgl32.Vec3{float32(state.Position.X), float32(state.Position.Y), float32(state.Position.Z)}
	result.Vel = mgl32.Vec3{float32(state.Velocity.X), float32(state.Velocity.Y), float32(state.Velocity.Z)}
	result.OnGround = state.OnGround
	result.CollideX = state.Collisions.X
	result.CollideY = state.Collisions.Y
	result.CollideZ = state.Collisions.Z
	return result
}

// liquidExitScripts records the bounded v0.1.5 liquid evidence slice.
func liquidExitScripts() []scenarioScript {
	ordinary := blockPhysics{Friction: 0.6, HorizontalSpeedFactor: 1, VerticalSpeedFactor: 1, SurfaceResponse: "none"}
	water := ordinary
	water.Flags = flagWater
	water.FluidHeightBlocks = 1
	ledge := aabb{Min: vec3{1, 0, 0}, Max: vec3{2, 1, 1}}
	waterFootprint := []physicsRegion{{Min: [3]int32{-1, 0, -1}, Max: [3]int32{1, 1, 2}, Physics: water}}
	ledgeInitial := playerState{Position: vec3{0.5, 0.5, 0.5}, Velocity: vec3{0.5, 0, 0}}
	openWaterInitial := playerState{Position: vec3{0.5, 0.5, 0.5}}
	input := movementInput{Jumping: true}
	build := func(name string, boxes []aabb, regions []physicsRegion, revision uint64) scenarioScript {
		world := scenarioWorld{Name: name + "_world", Origin: [3]int32{0, 0, 0}, Revision: revision, Boxes: boxes, Physics: ordinary, PhysicsRegions: regions}
		return observedScript(name, ledgeInitial, []observedStep{{world: world, input: input}})
	}
	openWater := scenarioWorld{Name: "open_water_held_ascent_world", Origin: [3]int32{0, 0, 0}, Revision: 104, Boxes: []aabb{}, Physics: ordinary, PhysicsRegions: []physicsRegion{{Min: [3]int32{-1, 0, -1}, Max: [3]int32{2, 2, 2}, Physics: water}}}
	return []scenarioScript{
		build("water_ledge_exit_boost", []aabb{ledge}, waterFootprint, 101),
		build("water_ledge_exit_blocked_above", []aabb{ledge, {Min: vec3{0, 2.5, 0}, Max: vec3{1, 3.5, 1}}}, waterFootprint, 102),
		build("water_ledge_exit_still_submerged", []aabb{ledge}, []physicsRegion{{Min: [3]int32{-1, 0, -1}, Max: [3]int32{1, 4, 2}, Physics: water}}, 103),
		observedScript("open_water_held_ascent", openWaterInitial, []observedStep{{world: openWater, input: input}}),
	}
}

// observedScript runs all steps against the pinned simulator and serialises its outputs.
func observedScript(name string, initial playerState, observed []observedStep) scenarioScript {
	state := toBedsimState(initial)
	steps := make([]scenarioStep, 0, len(observed))
	for index, step := range observed {
		simulator := bedsim.Simulator{World: scriptedWorld{world: step.world}, Options: bedsim.SimulationOptions{RequireLiquidLayer: true}}
		before := state
		result := simulator.Simulate(&state, toBedsimInput(state, step.input))
		expected := tickResult{Tick: uint64(index + 1), Position: fromVec3(result.Position), Velocity: fromVec3(result.Velocity), Movement: fromVec3(result.Movement), Collisions: collisions{X: result.CollideX, Y: result.CollideY, Z: result.CollideZ}, OnGround: result.OnGround, Environment: environment(step.world, before), WorldIdentity: identity(step.world)}
		steps = append(steps, scenarioStep{World: step.world, Input: step.input, Expected: &expected})
	}
	return scenarioScript{Scenario: name, Evidence: scenarioEvidence{Status: "bedsim_observed_with_manifest_context"}, Initial: initial, Steps: steps}
}

// toBedsimInput maps the fixture's held controls onto the public bedsim input state.
func toBedsimInput(before bedsim.MovementState, input movementInput) bedsim.InputState {
	return bedsim.InputState{MoveVector: mgl32.Vec2{float32(input.Strafe), float32(input.Forward)}, Yaw: float32(input.YawDegrees), HeadYaw: float32(input.YawDegrees), ClientPos: before.Pos, ClientVel: before.Vel, StartSprinting: input.Sprinting && !before.Sprinting, StopSprinting: !input.Sprinting && before.Sprinting, SprintDown: input.Sprinting, StartJumping: input.JumpPressed, Jumping: input.Jumping, Sneaking: input.Sneaking, SneakDown: input.Sneaking}
}

// fromVec3 widens bedsim's float32 output for lossless JSON representation.
func fromVec3(value mgl32.Vec3) vec3 {
	return vec3{X: float64(value.X()), Y: float64(value.Y()), Z: float64(value.Z())}
}

// environment reports only the environment facts that are representable by
// Cinnabar's current public simulation state.
func environment(world scenarioWorld, state bedsim.MovementState) movementEnvironment {
	water := false
	for _, region := range world.PhysicsRegions {
		if region.Physics.Flags&flagWater != 0 && state.Pos.X() >= float32(region.Min[0]) && state.Pos.X() < float32(region.Max[0]) && state.Pos.Y() < float32(region.Max[1]) && state.Pos.Y()+1.8 > float32(region.Min[1]) && state.Pos.Z() >= float32(region.Min[2]) && state.Pos.Z() < float32(region.Max[2]) {
			water = true
		}
	}
	return movementEnvironment{InWater: water, HorizontalSpeedFactor: 1, VerticalSpeedFactor: 1, SurfaceResponse: "none"}
}

// identity binds the fixture geometry and every regioned liquid fact to its
// deterministic collision-world identity.
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
	hashPhysics(hash, scratch[:], world.Physics)
	if world.Unloaded {
		hash.Write([]byte{1})
	} else {
		hash.Write([]byte{0})
	}
	if len(world.PhysicsRegions) != 0 {
		binary.LittleEndian.PutUint32(scratch[:4], uint32(len(world.PhysicsRegions)))
		hash.Write(scratch[:4])
		for _, region := range world.PhysicsRegions {
			for _, coordinate := range append(region.Min[:], region.Max[:]...) {
				binary.LittleEndian.PutUint32(scratch[:4], uint32(coordinate))
				hash.Write(scratch[:4])
			}
			hashPhysics(hash, scratch[:], region.Physics)
		}
	}
	var digest [32]uint8
	copy(digest[:], hash.Sum(nil))
	return worldIdentity{Protocol: 1001, IDSpace: "sequential", PregSHA256: digest, Chunks: []identityChunk{{Dimension: 0, X: world.Origin[0] >> 4, Z: world.Origin[2] >> 4, Revision: world.Revision}}}
}

// hashPhysics writes the compact physics representation used by the Rust
// scenario-world identity contract.
func hashPhysics(hash io.Writer, scratch []byte, physics blockPhysics) {
	for _, value := range []float64{physics.Friction, physics.HorizontalSpeedFactor, physics.VerticalSpeedFactor, physics.FluidHeightBlocks} {
		binary.LittleEndian.PutUint64(scratch, math.Float64bits(value))
		_, _ = hash.Write(scratch)
	}
	_, _ = hash.Write([]byte{physics.Flags, 0})
}
