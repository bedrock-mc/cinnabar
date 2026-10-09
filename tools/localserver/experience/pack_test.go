package experience

import (
	"archive/zip"
	"bytes"
	"encoding/json"
	"fmt"
	"image"
	"image/png"
	"io"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/world"
)

// visualExperience writes the files of the visual Experience id into dir and returns its Loaded,
// as the runtime would load it: a drive, two slot states shown by bones and the facing trait
// turning it, and with controller a controller whose online state swaps its geometry for one with
// an animated material.
func visualExperience(dir, id string, controller bool) (Loaded, error) {
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return Loaded{}, err
	}
	write := func(name string, data []byte) (string, error) {
		path := filepath.Join(dir, name)
		return path, os.WriteFile(path, data, 0o644)
	}
	geometry := func(name string, bones ...any) []byte {
		data, _ := json.Marshal(map[string]any{
			"format_version": "1.21.0",
			"minecraft:geometry": []any{map[string]any{
				"description": map[string]any{"identifier": "geometry." + id + "." + name},
				"bones":       bones,
			}},
		})
		return data
	}
	boxBone := func(name string) any {
		return map[string]any{"name": name, "cubes": []any{map[string]any{"uv": []any{0, 0}}}}
	}
	lightsBone := map[string]any{"name": "body", "cubes": []any{map[string]any{"uv": map[string]any{
		"north": map[string]any{"uv": []any{0, 0}, "material_instance": "lights"},
		"south": map[string]any{"uv": []any{0, 0}, "material_instance": "lights"},
	}}}}
	files := map[string][]byte{
		"drive.geo.json":              geometry("drive", boxBone("base"), boxBone("slot_0"), boxBone("slot_1")),
		"controller_offline.geo.json": geometry("controller_offline", boxBone("body")),
		"controller_online.geo.json":  geometry("controller_online", lightsBone),
		"side.png":                    mustPNG(16, 16),
		"lights.png":                  mustPNG(16, 32),
	}
	paths := make(map[string]string, len(files))
	for name, data := range files {
		path, err := write(name, data)
		if err != nil {
			return Loaded{}, err
		}
		paths[name] = path
	}
	side := []Material{{Instance: allFaces, Path: paths["side.png"], RenderMethod: RenderOpaque}}
	slot := func(i int) BoneVisibility {
		return BoneVisibility{
			Bone:    fmt.Sprintf("slot_%d", i),
			Visible: Condition{{{State: BlockState{Name: fmt.Sprintf("%s:slot_%d", id, i), Value: boolValue(true)}, Equal: true}}},
		}
	}
	drive := paths["drive.geo.json"]
	blocks := []BlockDef{{
		ID:          id + ":drive",
		DisplayName: "Drive",
		Mining:      Mining{Breakable: &Breakable{Hardness: 1}},
		States: []StateDef{
			{Name: id + ":slot_0", Values: StateValues{Bool: true}},
			{Name: id + ":slot_1", Values: StateValues{Bool: true}},
		},
		Placement: []PlacementState{PlacementFacingDirection},
		Visual: &Visual{
			Geometry: &drive, Materials: side, Bones: []BoneVisibility{slot(0), slot(1)},
		},
		Permutations: []Permutation{{When: Condition{{facingIs("east")}}, Rotation: &QuarterTurns{Y: 1}}},
	}}
	if controller {
		offline, online := paths["controller_offline.geo.json"], paths["controller_online.geo.json"]
		lights := []Material{{
			Instance: "lights", Path: paths["lights.png"], RenderMethod: RenderOpaque,
			Flipbook: &Flipbook{TicksPerFrame: 10, BlendFrames: true},
		}}
		state := id + ":state"
		blocks = append(blocks, BlockDef{
			ID:          id + ":controller",
			DisplayName: "Controller",
			Mining:      Mining{Breakable: &Breakable{Hardness: 1}},
			States:      []StateDef{{Name: state, Values: StateValues{Choices: &[]string{"offline", "online"}}}},
			Visual:      &Visual{Geometry: &offline, Materials: side},
			Permutations: []Permutation{{
				When:      Condition{{{State: BlockState{Name: state, Value: choiceValue("online")}, Equal: true}}},
				Geometry:  &online,
				Materials: &lights,
			}},
		})
	}
	return Loaded{Protocol: 5, ID: id, Version: "0.1.0", Blocks: blocks, Focus: true}, nil
}

// mustPNG encodes a blank w×h PNG for a fixture built outside a test.
func mustPNG(w, h int) []byte {
	var out bytes.Buffer
	if err := png.Encode(&out, image.NewNRGBA(image.Rect(0, 0, w, h))); err != nil {
		panic(err)
	}
	return out.Bytes()
}

// packFiles reads every file of the resource pack that the test server built.
func packFiles(t *testing.T) map[string][]byte {
	t.Helper()
	registered(t)
	pack := registration.pack
	if pack == nil {
		t.Fatal("the server built no resource pack")
	}
	r, err := zip.NewReader(pack, int64(pack.Len()))
	if err != nil {
		t.Fatal(err)
	}
	files := make(map[string][]byte)
	for _, f := range r.File {
		rc, err := f.Open()
		if err != nil {
			t.Fatal(err)
		}
		data, err := io.ReadAll(rc)
		rc.Close()
		if err != nil {
			t.Fatal(err)
		}
		files[f.Name] = data
	}
	return files
}

// The pack gate (SP5): Dragonfly's pack carries each block's geometries under its whole id, so
// two Experiences' drives do not collide, the further geometries of permutations, and the
// flipbooks; every state combination is a registered block.
func TestVisualBlocksReachThePack(t *testing.T) {
	files := packFiles(t)
	for _, path := range []string{
		"models/blocks/visual/drive.geo.json",
		"models/blocks/visual2/drive.geo.json",
		"models/blocks/visual/controller.geo.json",
		"models/blocks/visual/controller/controller_online.geo.json",
	} {
		if _, ok := files[path]; !ok {
			t.Errorf("the pack has no %s", path)
		}
	}
	var drives [2]struct {
		Geometry []struct {
			Description struct{ Identifier string } `json:"description"`
		} `json:"minecraft:geometry"`
	}
	for i, id := range []string{"visual", "visual2"} {
		if err := json.Unmarshal(files["models/blocks/"+id+"/drive.geo.json"], &drives[i]); err != nil {
			t.Fatal(err)
		}
		if got := drives[i].Geometry[0].Description.Identifier; got != "geometry."+id+".drive" {
			t.Errorf("%s's drive geometry is %q", id, got)
		}
	}

	var flipbooks []map[string]any
	if err := json.Unmarshal(files["textures/flipbook_textures.json"], &flipbooks); err != nil {
		t.Fatalf("flipbook_textures.json: %v", err)
	}
	controller, _ := registered(t).Lookup("visual:controller")
	var online []string
	for key := range controller.Flipbooks() {
		online = append(online, key)
	}
	if len(online) != 1 || len(flipbooks) != 1 || flipbooks[0]["atlas_tile"] != online[0] ||
		flipbooks[0]["ticks_per_frame"] != float64(10) || flipbooks[0]["blend_frames"] == false {
		t.Errorf("flipbooks = %v, the controller animates %v", flipbooks, online)
	}
	if _, ok := files["textures/blocks/"+online[0]+".png"]; !ok {
		t.Errorf("the pack lacks the animated texture %s", online[0])
	}

	for _, facing := range []string{"down", "up", "north", "south", "west", "east"} {
		for _, slots := range [][2]bool{{false, false}, {true, false}, {false, true}, {true, true}} {
			props := map[string]any{
				"minecraft:facing_direction": facing, "visual:slot_0": slots[0], "visual:slot_1": slots[1],
			}
			b, ok := world.BlockByName("visual:drive", props)
			if !ok {
				t.Fatalf("no registered drive %v", props)
			}
			if _, got := b.EncodeBlock(); jsonOf(got) != jsonOf(props) {
				t.Fatalf("drive %v encodes as %v", props, got)
			}
		}
	}
}

// The network form of a block's definition carries the visual through the fork: its geometry
// with bone visibility, its states sorted by name, its trait, and its permutations as Molang
// over the states.
func TestVisualBlockComponents(t *testing.T) {
	drive, _ := registered(t).Lookup("visual:drive")
	entry := server.CustomBlockEntry(drive, 1)
	props := entry.Properties
	components := props["components"].(map[string]any)
	geometry := components["minecraft:geometry"].(map[string]any)
	if geometry["identifier"] != "geometry.visual.drive" {
		t.Errorf("geometry %v", geometry["identifier"])
	}
	wantBones := map[string]any{
		"slot_0": "q.block_state('visual:slot_0') == true",
		"slot_1": "q.block_state('visual:slot_1') == true",
	}
	if jsonOf(geometry["bone_visibility"]) != jsonOf(wantBones) {
		t.Errorf("bone_visibility = %v, want %v", geometry["bone_visibility"], wantBones)
	}
	var names []string
	for _, p := range props["properties"].([]map[string]any) {
		names = append(names, p["name"].(string))
	}
	if !slices.Equal(names, []string{"visual:slot_0", "visual:slot_1"}) {
		t.Errorf("properties = %v", names)
	}
	traits := props["traits"].([]map[string]any)
	if len(traits) != 1 || traits[0]["name"] != "minecraft:placement_direction" {
		t.Fatalf("traits = %v", traits)
	}
	if enabled := traits[0]["enabled_states"].(map[string]any); enabled["facing_direction"] != uint8(1) ||
		enabled["cardinal_direction"] != uint8(0) {
		t.Errorf("enabled states = %v", enabled)
	}
	permutations := props["permutations"].([]map[string]any)
	if len(permutations) != 1 ||
		permutations[0]["condition"] != "q.block_state('minecraft:facing_direction') == 'east'" {
		t.Fatalf("permutations = %v", permutations)
	}
	turn := permutations[0]["components"].(map[string]any)["minecraft:transformation"].(map[string]any)
	if turn["RY"] != int32(1) || turn["RX"] != int32(0) {
		t.Errorf("transformation = %v", turn)
	}

	controller, _ := registered(t).Lookup("visual:controller")
	cprops := server.CustomBlockEntry(controller, 2).Properties
	perm := cprops["permutations"].([]map[string]any)[0]
	pc := perm["components"].(map[string]any)
	if g := pc["minecraft:geometry"].(map[string]any); g["identifier"] != "geometry.visual.controller_online" {
		t.Errorf("online geometry = %v", g)
	}
	materials := pc["minecraft:material_instances"].(map[string]any)["materials"].(map[string]any)
	lights := materials["lights"].(map[string]any)
	if _, ok := controller.Flipbooks()[lights["texture"].(string)]; !ok {
		t.Errorf("the online lights use %v, which no flipbook animates", lights["texture"])
	}
	if base := cprops["components"].(map[string]any)["minecraft:geometry"].(map[string]any); base["identifier"] != "geometry.visual.controller_offline" {
		t.Errorf("base geometry = %v", base)
	}
}

// Each Experience's item icon reaches the pack under its whole id, so the probe's cell and
// other's copy keep one each, and the item atlas points at them.
func TestItemIconsReachThePack(t *testing.T) {
	files := packFiles(t)
	var atlas struct {
		TextureData map[string]struct {
			Textures string `json:"textures"`
		} `json:"texture_data"`
	}
	if err := json.Unmarshal(files["textures/item_texture.json"], &atlas); err != nil {
		t.Fatal(err)
	}
	for _, id := range []string{probeCell, "other:cell"} {
		path := atlas.TextureData[id].Textures
		if _, ok := files[path]; !ok || !strings.Contains(path, strings.Replace(id, ":", "/", 1)) {
			t.Errorf("%s's icon is %q, which the pack has: %v", id, path, ok)
		}
	}
}
