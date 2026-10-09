package experience

import (
	"encoding/json"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/block/customblock"
	"github.com/go-gl/mathgl/mgl64"
)

// visualAssets writes the assets of the rule tests to a temporary directory, as the runtime's
// load tests do: the 1×1 counter.png, the flipbook strips strip.png (16×48, three frames) and
// ragged.png (16×40), and the geometries cable.geo.json (geometry.probe.cable: bones core,
// north and slot_0 to slot_64, drawn with base and the faces of core's box UV), alt.geo.json
// (another file of geometry.probe.cable), foreign.geo.json (geometry.other.cable), two.geo.json
// (two geometries) and post.geo.json (geometry.probe.post, drawn with post alone). It returns
// each file's absolute path by name.
func visualAssets(t *testing.T) map[string]string {
	t.Helper()
	dir := t.TempDir()
	geometry := func(identifier string, bones []any) []byte {
		data, err := json.Marshal(map[string]any{
			"format_version": "1.21.0",
			"minecraft:geometry": []any{map[string]any{
				"description": map[string]any{"identifier": identifier},
				"bones":       bones,
			}},
		})
		if err != nil {
			t.Fatal(err)
		}
		return data
	}
	cable := []any{
		map[string]any{"name": "core", "cubes": []any{map[string]any{"uv": []any{0, 0}}}},
		map[string]any{"name": "north", "cubes": []any{map[string]any{"uv": map[string]any{
			"north": map[string]any{"uv": []any{0, 0}, "material_instance": "base"},
			"up":    map[string]any{"uv": []any{0, 0}, "material_instance": "base"},
		}}}},
	}
	for i := 0; i <= maxBones; i++ {
		cable = append(cable, map[string]any{"name": fmt.Sprintf("slot_%d", i)})
	}
	post := []any{map[string]any{"name": "post", "cubes": []any{map[string]any{"uv": map[string]any{
		"up": map[string]any{"uv": []any{0, 0}, "material_instance": "post"},
	}}}}}
	var two map[string]any
	if err := json.Unmarshal(geometry("geometry.probe.two", nil), &two); err != nil {
		t.Fatal(err)
	}
	geometries := two["minecraft:geometry"].([]any)
	two["minecraft:geometry"] = append(geometries, geometries[0])
	twoData, err := json.Marshal(two)
	if err != nil {
		t.Fatal(err)
	}
	files := map[string][]byte{
		"counter.png":      encodePNG(t, 1, 1),
		"strip.png":        encodePNG(t, 16, 48),
		"ragged.png":       encodePNG(t, 16, 40),
		"cable.geo.json":   geometry("geometry.probe.cable", cable),
		"alt.geo.json":     geometry("geometry.probe.cable", nil),
		"foreign.geo.json": geometry("geometry.other.cable", nil),
		"two.geo.json":     twoData,
		"post.geo.json":    geometry("geometry.probe.post", post),
	}
	paths := make(map[string]string, len(files))
	for name, data := range files {
		path := filepath.Join(dir, name)
		if err := os.WriteFile(path, data, 0o644); err != nil {
			t.Fatal(err)
		}
		paths[name] = path
	}
	return paths
}

func boolState(name string) StateDef {
	return StateDef{Name: "probe:" + name, Values: StateValues{Bool: true}}
}

func choiceState(name string, values ...string) StateDef {
	return StateDef{Name: "probe:" + name, Values: StateValues{Choices: &values}}
}

func boolValue(v bool) StateValue     { return StateValue{Bool: &v} }
func choiceValue(v string) StateValue { return StateValue{Choice: &v} }

// is is name == v for one of the probe's bool states.
func is(name string, v bool) StateTest {
	return StateTest{State: BlockState{Name: "probe:" + name, Value: boolValue(v)}, Equal: true}
}

func facingIs(v string) StateTest {
	return StateTest{State: BlockState{Name: "minecraft:facing_direction", Value: choiceValue(v)}, Equal: true}
}

func box(lo, hi float32) *PixelBox {
	return &PixelBox{Min: Pixel{lo, lo, lo}, Max: Pixel{hi, hi, hi}}
}

// visualCable is a valid cable shaped like the converter's, as visualAssets' files make it: six
// connection states, the facing trait, the cable geometry with a bone shown by a condition over
// two states, a flipbook, boxes, and a permutation that turns it by the trait's state and one
// that swaps its geometry.
func visualCable(assets map[string]string) BlockDef {
	cable, post := assets["cable.geo.json"], assets["post.geo.json"]
	var states []StateDef
	for _, name := range []string{"down", "up", "north", "south", "west", "east"} {
		states = append(states, boolState(name))
	}
	postMaterials := []Material{{Instance: "post", Path: assets["counter.png"], RenderMethod: RenderOpaque}}
	return BlockDef{
		ID:          "probe:cable",
		DisplayName: "Probe Cable",
		Mining:      Mining{Breakable: &Breakable{Hardness: 1}},
		States:      states,
		Placement:   []PlacementState{PlacementFacingDirection},
		Visual: &Visual{
			Geometry: &cable,
			Materials: []Material{
				{
					Instance: "base", Path: assets["strip.png"], RenderMethod: RenderAlphaTest,
					Flipbook: &Flipbook{TicksPerFrame: 25, Frames: []uint32{0, 1, 2, 1}, BlendFrames: true},
				},
				{Instance: allFaces, Path: assets["counter.png"], RenderMethod: RenderAlphaTest},
			},
			Bones: []BoneVisibility{{
				Bone:    "north",
				Visible: Condition{{is("north", true)}, {is("south", false)}},
			}},
			Collision: box(6, 10),
			Selection: box(0, 16),
		},
		Permutations: []Permutation{
			{When: Condition{{facingIs("east")}}, Rotation: &QuarterTurns{Y: 1}},
			{
				When:      Condition{{is("north", false), is("up", true)}},
				Geometry:  &post,
				Materials: &postMaterials,
			},
		},
	}
}

// Each row changes the valid cable in one way; the adapter refuses what the runtime refuses
// (crates/experience-runtime/src/load/tests.rs), naming the Experience and the block.
func TestBlockTypeRules(t *testing.T) {
	assets := visualAssets(t)
	ptr := func(s string) *string { return &s }
	values := func(n int) StateDef {
		var vs []string
		for i := range n {
			vs = append(vs, fmt.Sprintf("v%d", i))
		}
		return choiceState("colour", vs...)
	}
	bools := func(n int) func(*BlockDef) {
		return func(b *BlockDef) {
			b.Placement = nil
			b.States = nil
			for i := range n {
				b.States = append(b.States, boolState(fmt.Sprintf("s%d", i)))
			}
			b.Permutations = nil
			b.Visual.Bones = nil
		}
	}
	slotBones := func(n int) func(*BlockDef) {
		return func(b *BlockDef) {
			b.Visual.Bones = nil
			for i := range n {
				b.Visual.Bones = append(b.Visual.Bones, BoneVisibility{
					Bone: fmt.Sprintf("slot_%d", i), Visible: Condition{{is("north", true)}},
				})
			}
		}
	}
	tests := func(n int) Condition {
		var clause []StateTest
		for range n {
			clause = append(clause, is("north", true))
		}
		return Condition{clause}
	}
	cases := []struct {
		name string
		edit func(*BlockDef)
		want string // empty when accepted
	}{
		{"the cable", func(*BlockDef) {}, ""},
		{"a state outside the namespace", func(b *BlockDef) { b.States[0].Name = "other:down" }, "is not probe:<name>"},
		{"a state named in uppercase", func(b *BlockDef) { b.States[0].Name = "probe:Down" }, "is not probe:<name>"},
		{"a state declared twice", func(b *BlockDef) { b.States = append(b.States, boolState("down")) }, "declared twice"},
		{"16 values", func(b *BlockDef) { b.States = append(b.States, values(maxStateValues)) }, ""},
		{"17 values", func(b *BlockDef) { b.States = append(b.States, values(maxStateValues+1)) }, "has 17 values"},
		{"no values", func(b *BlockDef) { b.States = append(b.States, values(0)) }, "has 0 values"},
		{"a value twice", func(b *BlockDef) { b.States = append(b.States, choiceState("colour", "red", "red")) }, "twice"},
		{"a value with a quote", func(b *BlockDef) { b.States = append(b.States, choiceState("colour", "it's")) }, "does not match"},
		{"2^16 combinations", bools(16), ""},
		{"2^17 combinations", bools(17), "more than 65536 combinations"},
		{"2^16 combinations and a trait", func(b *BlockDef) {
			bools(16)(b)
			b.Placement = []PlacementState{PlacementFacingDirection}
		}, "more than 65536 combinations"},
		{"a condition on a state the block lacks", func(b *BlockDef) {
			b.Visual.Bones[0].Visible = Condition{{is("sideways", true)}}
		}, "does not have"},
		{"a condition on a trait the block lacks", func(b *BlockDef) {
			b.Placement = []PlacementState{PlacementBlockFace}
		}, `"minecraft:facing_direction", which the block does not have`},
		{"a bool state tested against a choice", func(b *BlockDef) {
			b.Visual.Bones[0].Visible = Condition{{{State: BlockState{Name: "probe:north", Value: choiceValue("yes")}, Equal: true}}}
		}, "a value it does not take"},
		{"a trait state tested against a value it lacks", func(b *BlockDef) {
			b.Permutations[0].When = Condition{{facingIs("sideways")}}
		}, "a value it does not take"},
		{"64 tests", func(b *BlockDef) { b.Visual.Bones[0].Visible = tests(maxConditionTests) }, ""},
		{"65 tests", func(b *BlockDef) { b.Visual.Bones[0].Visible = tests(maxConditionTests + 1) }, "65 tests"},
		{"a geometry outside the namespace", func(b *BlockDef) { b.Visual.Geometry = ptr(assets["foreign.geo.json"]) }, `outside namespace "geometry.probe."`},
		{"a missing geometry", func(b *BlockDef) { b.Visual.Geometry = ptr(assets["cable.geo.json"] + ".missing") }, "geometry"},
		{"a file of two geometries", func(b *BlockDef) { b.Visual.Geometry = ptr(assets["two.geo.json"]) }, "holds 2 geometries"},
		{"one identifier in two files", func(b *BlockDef) { b.Permutations[1].Geometry = ptr(assets["alt.geo.json"]) }, "in two files"},
		{"a bone the geometry lacks", func(b *BlockDef) { b.Visual.Bones[0].Bone = "south" }, `has no bone "south"`},
		{"64 bones", slotBones(maxBones), ""},
		{"65 bones", slotBones(maxBones + 1), "65 bones"},
		{"a bone shown twice", func(b *BlockDef) { b.Visual.Bones = append(b.Visual.Bones, b.Visual.Bones[0]) }, "two visibilities"},
		{"a bone on the full cube", func(b *BlockDef) {
			b.Visual.Geometry = nil
			b.Visual.Materials = b.Visual.Materials[1:]
			b.Permutations = b.Permutations[:1]
		}, "the full cube has no bones"},
		{"an instance the geometry does not draw with", func(b *BlockDef) {
			b.Visual.Materials = append(b.Visual.Materials, Material{Instance: "lid", Path: assets["counter.png"], RenderMethod: RenderOpaque})
		}, `material instance "lid" is neither`},
		{"an instance twice", func(b *BlockDef) { b.Visual.Materials = append(b.Visual.Materials, b.Visual.Materials[1]) }, "listed twice"},
		{"an instance without a material and no *", func(b *BlockDef) { b.Visual.Materials = b.Visual.Materials[:1] }, "has no material"},
		{"a missing material", func(b *BlockDef) { b.Visual.Materials[1].Path = assets["counter.png"] + ".missing" }, "counter.png.missing"},
		{"no materials", func(b *BlockDef) { b.Visual.Materials = nil }, "it lists 0 materials"},
		{"a ragged flipbook strip", func(b *BlockDef) { b.Visual.Materials[0].Path = assets["ragged.png"] }, "not whole square frames"},
		{"a flipbook frame past the strip", func(b *BlockDef) { b.Visual.Materials[0].Flipbook.Frames = []uint32{0, 3} }, "shows frame 3 of a strip of 3"},
		{"a flipbook frame of 0 ticks", func(b *BlockDef) { b.Visual.Materials[0].Flipbook.TicksPerFrame = 0 }, "0 ticks"},
		{"a flipbook of every frame", func(b *BlockDef) { b.Visual.Materials[0].Flipbook.Frames = nil }, ""},
		{"a box past the block", func(b *BlockDef) { b.Visual.Collision = box(0, 17) }, "collision box spans 0 to 17"},
		{"an empty box", func(b *BlockDef) { b.Visual.Selection = box(4, 4) }, "selection box spans 4 to 4"},
		{"a box at NaN", func(b *BlockDef) { b.Visual.Collision = box(float32(math.NaN()), 4) }, "collision box spans NaN"},
		{"four quarter turns", func(b *BlockDef) { b.Visual.Rotation = &QuarterTurns{Y: 4} }, "each axis takes 0 to 3"},
		{"textures beside a visual", func(b *BlockDef) {
			b.Textures = []Texture{{Slot: allFaces, Path: assets["counter.png"]}}
		}, "binds textures and declares a visual"},
		{"permutations without a visual", func(b *BlockDef) {
			b.Visual = nil
			b.Textures = []Texture{{Slot: allFaces, Path: assets["counter.png"]}}
		}, "no visual"},
		{"a permutation that never holds", func(b *BlockDef) { b.Permutations[0].When = nil }, "never holds"},
		{"a permutation whose clause always holds", func(b *BlockDef) { b.Permutations[0].When = Condition{{}} }, ""},
		{"a permutation that sets nothing", func(b *BlockDef) { b.Permutations[0].Rotation = nil }, "sets nothing"},
		{"64 permutations", func(b *BlockDef) {
			b.Permutations = slices.Repeat(b.Permutations[:1], maxPermutations)
		}, ""},
		{"65 permutations", func(b *BlockDef) {
			b.Permutations = slices.Repeat(b.Permutations[:1], maxPermutations+1)
		}, "65 permutations"},
		{"a permutation geometry the materials do not cover", func(b *BlockDef) {
			b.Visual.Materials = b.Visual.Materials[:1]
			for _, face := range []string{"up", "down", "north", "south", "east", "west"} {
				b.Visual.Materials = append(b.Visual.Materials, Material{Instance: face, Path: assets["counter.png"], RenderMethod: RenderOpaque})
			}
			b.Permutations[1].Materials = nil
		}, `material instance "post" is drawn but has no material`},
		{"a permutation bone of its own geometry", func(b *BlockDef) {
			b.Permutations[1].Bones = &[]BoneVisibility{{Bone: "post", Visible: Condition{{is("up", true)}}}}
		}, ""},
		{"a permutation bone of the visual's geometry under its own", func(b *BlockDef) {
			b.Permutations[1].Bones = &[]BoneVisibility{{Bone: "north"}}
		}, `geometry "geometry.probe.post" has no bone "north"`},
		{"a zero permutation rotation under a turned visual", func(b *BlockDef) {
			b.Visual.Rotation = &QuarterTurns{Y: 2}
			b.Permutations[0].Rotation = &QuarterTurns{}
		}, "cannot replace the visual's rotation"},
		{"a network member", func(b *BlockDef) { b.Network = true }, ""},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			def := visualCable(assets)
			c.edit(&def)
			_, err := newBlockType("probe", def, newAssetCache())
			switch {
			case c.want == "" && err != nil:
				t.Fatalf("refused: %v", err)
			case c.want != "" && err == nil:
				t.Fatalf("accepted, want %q", c.want)
			case c.want != "" && (!strings.Contains(err.Error(), c.want) ||
				!strings.Contains(err.Error(), `experience "probe": block "probe:cable"`)):
				t.Fatalf("%v lacks the block or %q", err, c.want)
			}
		})
	}
}

// Conditions become the Molang that vanilla's own definitions use: q.block_state against a
// bool or a quoted string, ands in parentheses inside an or.
func TestConditionMolang(t *testing.T) {
	for _, c := range []struct {
		when Condition
		want string
	}{
		{nil, "false"},
		{Condition{{}}, "true"},
		{Condition{{is("north", true)}}, "q.block_state('probe:north') == true"},
		{Condition{{facingIs("east")}}, "q.block_state('minecraft:facing_direction') == 'east'"},
		{
			Condition{{is("north", true), {State: BlockState{Name: "probe:up", Value: boolValue(false)}}}, {facingIs("up")}},
			"(q.block_state('probe:north') == true && q.block_state('probe:up') != false) || " +
				"q.block_state('minecraft:facing_direction') == 'up'",
		},
		{Condition{{}, {is("up", true)}}, "true || q.block_state('probe:up') == true"},
	} {
		if got := molang(c.when); got != c.want {
			t.Errorf("molang(%v) = %q, want %q", c.when, got, c.want)
		}
	}
}

// Every combination of a block's states is one Block, with its own hash and properties, and its
// states, placement traits' first, read back as the snapshot carries them.
func TestBlockStatesEnumerateCombinations(t *testing.T) {
	assets := visualAssets(t)
	typ, err := newBlockType("probe", visualCable(assets), newAssetCache())
	if err != nil {
		t.Fatal(err)
	}
	if typ.combinations != 6*64 {
		t.Fatalf("%d combinations, want %d", typ.combinations, 6*64)
	}
	hashes := make(map[uint64]bool)
	for s := range typ.combinations {
		b := Block{typ, s}
		_, state := b.Hash()
		if hashes[state] {
			t.Fatalf("state %d hashes like another", s)
		}
		hashes[state] = true
		id, props := b.EncodeBlock()
		if id != "probe:cable" || len(props) != 7 {
			t.Fatalf("state %d encodes as %s %v", s, id, props)
		}
		again, err := Block{typ, 0}.withStates(b.states())
		if err != nil || again != b {
			t.Fatalf("state %d read back as %v, %v", s, again.s, err)
		}
	}
	def := Block{typ, 0}
	want := BlockStates{
		{Name: "minecraft:facing_direction", Value: choiceValue("down")},
		{Name: "probe:down", Value: boolValue(false)},
		{Name: "probe:up", Value: boolValue(false)},
		{Name: "probe:north", Value: boolValue(false)},
		{Name: "probe:south", Value: boolValue(false)},
		{Name: "probe:west", Value: boolValue(false)},
		{Name: "probe:east", Value: boolValue(false)},
	}
	if got := def.states(); jsonOf(got) != jsonOf(want) {
		t.Errorf("default states = %s, want %s", jsonOf(got), jsonOf(want))
	}
	lit, err := def.withStates(BlockStates{{Name: "probe:north", Value: boolValue(true)}, {Name: "minecraft:facing_direction", Value: choiceValue("east")}})
	if err != nil {
		t.Fatal(err)
	}
	_, props := lit.EncodeBlock()
	if props["probe:north"] != true || props["minecraft:facing_direction"] != "east" || props["probe:up"] != false {
		t.Errorf("properties = %v", props)
	}
	for _, bad := range []BlockStates{
		{{Name: "probe:sideways", Value: boolValue(true)}},
		{{Name: "probe:north", Value: choiceValue("yes")}},
		{{Name: "minecraft:facing_direction", Value: choiceValue("sideways")}},
	} {
		if _, err := def.withStates(bad); err == nil {
			t.Errorf("withStates(%s) accepted", jsonOf(bad))
		}
	}
}

// The placement traits' states follow the vanilla client's minecraft:placement_direction and
// minecraft:placement_position traits: cardinal_direction from the yaw in quarters starting south; facing_direction down or up when
// the block is below the placer's feet or above its head within one block horizontally, else
// from the yaw like cardinal_direction; block_face the clicked face; vertical_half top for the
// clicked bottom face or a click above the middle.
func TestPlacementStates(t *testing.T) {
	traits := []PlacementState{
		PlacementCardinalDirection, PlacementFacingDirection, PlacementBlockFace, PlacementVerticalHalf,
	}
	at := placer{pos: mgl64.Vec3{0.5, 64, 0.5}, minY: 64, maxY: 65.8}
	for _, c := range []struct {
		name  string
		p     placer
		pos   cube.Pos
		face  cube.Face
		click mgl64.Vec3
		want  [4]string
	}{
		{"yaw 0 looks south", at, cube.Pos{5, 64, 0}, cube.FaceWest, mgl64.Vec3{1, 0.2, 0.5},
			[4]string{"south", "south", "west", "bottom"}},
		{"yaw 90 looks west", placer{yaw: 90, pos: at.pos, minY: 64, maxY: 65.8}, cube.Pos{5, 64, 0}, cube.FaceNorth, mgl64.Vec3{0.5, 0.7, 0},
			[4]string{"west", "west", "north", "top"}},
		{"yaw 180 looks north", placer{yaw: 180, pos: at.pos, minY: 64, maxY: 65.8}, cube.Pos{5, 64, 0}, cube.FaceUp, mgl64.Vec3{0.5, 1, 0.5},
			[4]string{"north", "north", "up", "bottom"}},
		{"yaw -90 looks east", placer{yaw: -90, pos: at.pos, minY: 64, maxY: 65.8}, cube.Pos{5, 64, 0}, cube.FaceDown, mgl64.Vec3{0.5, 0, 0.5},
			[4]string{"east", "east", "down", "top"}},
		{"yaw 44 rounds to south", placer{yaw: 44, pos: at.pos, minY: 64, maxY: 65.8}, cube.Pos{5, 64, 0}, cube.FaceSouth, mgl64.Vec3{},
			[4]string{"south", "south", "south", "bottom"}},
		{"a block under the feet faces down", at, cube.Pos{1, 63, 1}, cube.FaceUp, mgl64.Vec3{},
			[4]string{"south", "down", "up", "bottom"}},
		{"a block over the head faces up", at, cube.Pos{0, 66, -1}, cube.FaceDown, mgl64.Vec3{},
			[4]string{"south", "up", "down", "top"}},
		{"a block under the feet two blocks away goes by the yaw", at, cube.Pos{2, 63, 0}, cube.FaceUp, mgl64.Vec3{},
			[4]string{"south", "south", "up", "bottom"}},
	} {
		t.Run(c.name, func(t *testing.T) {
			for i, trait := range traits {
				if got := c.p.value(trait, c.pos, c.face, c.click); got != c.want[i] {
					t.Errorf("%s = %q, want %q", trait, got, c.want[i])
				}
			}
		})
	}
}

// The adapter's traits are the fork's: the states the runtime lists for each placement state are
// those the fork's traits add, values in the same order.
func TestPlacementValuesMatchFork(t *testing.T) {
	var forkStates []customblock.TraitState
	forkStates = append(forkStates, customblock.PlacementDirection{CardinalDirection: true, FacingDirection: true}.States()...)
	forkStates = append(forkStates, customblock.PlacementPosition{BlockFace: true, VerticalHalf: true}.States()...)
	enums := rustEnums(t)
	if len(enums.PlacementValues) != len(forkStates) {
		t.Fatalf("%d placement states, the fork has %d", len(enums.PlacementValues), len(forkStates))
	}
	for i, rust := range enums.PlacementValues {
		fork := forkStates[i]
		if rust.Placement != placementStates[i] || rust.State != fork.Name || jsonOf(rust.Values) != jsonOf(fork.Values) {
			t.Errorf("placement %d: Rust %s %s %v, fork %s %v", i, rust.Placement, rust.State, rust.Values, fork.Name, fork.Values)
		}
	}
}

// TestCubePermutationEmptyBones keeps a full cube when its permutation supplies no bone overrides.
func TestCubePermutationEmptyBones(t *testing.T) {
	assets := visualAssets(t)
	bones := []BoneVisibility{}
	def := BlockDef{
		ID: "probe:lamp", DisplayName: "Probe Lamp",
		Mining:       Mining{Breakable: &Breakable{Hardness: 1}},
		States:       []StateDef{boolState("on")},
		Visual:       &Visual{Materials: []Material{{Instance: allFaces, Path: assets["counter.png"], RenderMethod: RenderOpaque}}},
		Permutations: []Permutation{{When: Condition{{is("on", true)}}, Bones: &bones}},
	}
	typ, err := newBlockType("probe", def, newAssetCache())
	if err != nil {
		t.Fatal(err)
	}
	block := Block{typ, 0}
	permutations := block.Permutations()
	if !block.Properties().Cube || len(permutations) != 1 || permutations[0].Properties.Geometry != "" || len(permutations[0].Properties.BoneVisibility) != 0 {
		t.Fatalf("empty bone overrides changed the full cube: %+v", permutations)
	}
	bones = []BoneVisibility{{Bone: "missing", Visible: Condition{{is("on", true)}}}}
	if _, err := newBlockType("probe", def, newAssetCache()); err == nil || !strings.Contains(err.Error(), "the full cube has no bones") {
		t.Fatalf("named bone on a cube was not refused: %v", err)
	}
}

// Zero rotation cannot undo an earlier matching rotation, but disjoint permutations remain valid.
func TestZeroRotationAfterPermutation(t *testing.T) {
	assets := visualAssets(t)
	for _, c := range []struct {
		name    string
		when    Condition
		refused bool
	}{
		{"matching", Condition{{is("on", true)}}, true},
		{"disjoint", Condition{{is("on", false)}}, false},
		{"negated match", Condition{{StateTest{State: BlockState{Name: "probe:on", Value: boolValue(false)}, Equal: false}}}, true},
		{"contradictory", Condition{{is("on", true), is("on", false)}}, false},
	} {
		t.Run(c.name, func(t *testing.T) {
			def := BlockDef{ID: "probe:lamp", DisplayName: "Probe Lamp", Mining: Mining{Breakable: &Breakable{Hardness: 1}}, States: []StateDef{boolState("on")},
				Visual:       &Visual{Materials: []Material{{Instance: allFaces, Path: assets["counter.png"], RenderMethod: RenderOpaque}}},
				Permutations: []Permutation{{When: Condition{{is("on", true)}}, Rotation: &QuarterTurns{Y: 1}}, {When: c.when, Rotation: &QuarterTurns{}}},
			}
			_, err := newBlockType("probe", def, newAssetCache())
			if c.refused && (err == nil || !strings.Contains(err.Error(), "zero rotation cannot replace an earlier permutation")) {
				t.Fatalf("%v; want the overlapping reset refused", err)
			}
			if !c.refused && err != nil {
				t.Fatalf("refused a disjoint reset: %v", err)
			}
		})
	}
}

// Clause intersection agrees with evaluation across every state of a small bool/choice block.
func TestConditionOverlapAgreesWithStates(t *testing.T) {
	def := testBlock("probe:lamp", writeTexture(t, "block.png", 1, 1))
	def.States = []StateDef{boolState("on"), choiceState("colour", "red", "green", "blue")}
	typ, err := newBlockType("probe", def, newAssetCache())
	if err != nil {
		t.Fatal(err)
	}
	red := StateTest{State: BlockState{Name: "probe:colour", Value: choiceValue("red")}, Equal: true}
	notRed := red
	notRed.Equal = false
	conditions := []Condition{nil, {{}}, {{is("on", true)}}, {{is("on", false)}}, {{red}}, {{notRed}}, {{red, notRed}}, {{is("on", true), red}}, {{is("on", false)}, {notRed}}}
	for i, left := range conditions {
		for j, right := range conditions {
			want := false
			for state := uint32(0); state < typ.combinations; state++ {
				b := Block{t: typ, s: state}
				want = want || (holds(left, b) && holds(right, b))
			}
			if got := conditionsOverlap(typ.axes, left, right); got != want {
				t.Errorf("conditions %d/%d overlap %v, want %v", i, j, got, want)
			}
		}
	}
}
