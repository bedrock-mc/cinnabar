package main

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"testing"
)

func TestEducationConstructionRetainsUnrelatedPaletteRecords(t *testing.T) {
	previousRoot := os.Getenv("CINNABAR_PREVIOUS_REGISTRY_ROOT")
	if previousRoot == "" {
		t.Skip("missing previous registry fixtures: set CINNABAR_PREVIOUS_REGISTRY_ROOT")
	}
	currentRoot := filepath.Join("..", "..", "crates", "assets", "data")
	read := func(root, name string) []byte {
		t.Helper()
		data, err := os.ReadFile(filepath.Join(root, name))
		if err != nil {
			t.Fatal(err)
		}
		return data
	}
	previousBREG, currentBREG := read(previousRoot, "block-registry-v2193.bin"), read(currentRoot, "block-registry-v2193.bin")
	_, previous, err := decodeBREGRecords(previousBREG, v2193BlockProtocol)
	if err != nil {
		t.Fatal(err)
	}
	_, current, err := decodeBREGRecords(currentBREG, v2193BlockProtocol)
	if err != nil {
		t.Fatal(err)
	}
	previousLight, err := decodeLREGProperties(read(previousRoot, "block-light-registry-v2193.bin"), previousBREG, v2193BlockProtocol, len(previous))
	if err != nil {
		t.Fatal(err)
	}
	currentLight, err := decodeLREGProperties(read(currentRoot, "block-light-registry-v2193.bin"), currentBREG, v2193BlockProtocol, len(current))
	if err != nil {
		t.Fatal(err)
	}
	if len(previous) != len(current) {
		t.Fatal("construction admission changed palette size")
	}
	previousPhysics := decodeV2193PhysicsArtifact(t, read(previousRoot, "block-physics-v2193.bin"), previousBREG, len(previous))
	currentPhysics := decodeV2193PhysicsArtifact(t, read(currentRoot, "block-physics-v2193.bin"), currentBREG, len(current))
	changed := 0
	for index, record := range current {
		before := previous[index]
		if record.SequentialID != before.SequentialID || record.NetworkHash != before.NetworkHash {
			t.Fatalf("construction admission changed palette identity at %d", index)
		}
		if !isEducationConstructionName(record.Name) {
			if !reflect.DeepEqual(record, before) || currentLight[index] != previousLight[index] || !reflect.DeepEqual(currentPhysics[index], previousPhysics[index]) {
				t.Fatalf("construction admission changed unrelated state %d (%s)", index, record.Name)
			}
			continue
		}
		if before.Name != retailReservedName || reflect.DeepEqual(record, before) || currentLight[index] == previousLight[index] {
			t.Fatalf("construction state %d did not replace its diagnostic record", index)
		}
		changed++
	}
	if changed != v2193EducationStateCount {
		t.Fatalf("construction admission changed %d states, want %d", changed, v2193EducationStateCount)
	}
}

func TestEducationConstructionProjectionKeepsPaletteIdentitiesWithoutCreativeItems(t *testing.T) {
	border := []byte(`{"wall_connection_type_east":{"type":"string","value":"short"},"wall_connection_type_north":{"type":"string","value":"tall"},"wall_connection_type_south":{"type":"string","value":"none"},"wall_connection_type_west":{"type":"string","value":"short"},"wall_post_bit":{"type":"byte","value":1}}`)
	source := []Record{
		{SequentialID: 0, NetworkHash: 10, Name: "minecraft:allow", StateJSON: []byte(`{}`)},
		{SequentialID: 1, NetworkHash: 11, Name: "minecraft:deny", StateJSON: []byte(`{}`)},
		{SequentialID: 2, NetworkHash: 12, Name: "minecraft:border_block", StateJSON: border},
		{SequentialID: 3, NetworkHash: 13, Name: "minecraft:element_1", StateJSON: []byte(`{}`)},
	}
	projection, stats, err := projectV2193BlocksForTest(source, nil, nil)
	if err != nil {
		t.Fatal(err)
	}
	if stats.education != 3 {
		t.Fatalf("construction states = %d", stats.education)
	}
	for index, identity := range source[:3] {
		record := projection.records[index]
		if record.Name != identity.Name || record.SequentialID != identity.SequentialID || record.NetworkHash != identity.NetworkHash || !bytes.Equal(record.StateJSON, identity.StateJSON) {
			t.Fatalf("construction identity changed at %d: %+v", index, record)
		}
		if record.CollisionSeed.Confidence != CollisionConfidenceNone {
			t.Fatal("render admission changed collision behavior")
		}
	}
	for _, record := range projection.records[:2] {
		if record.ModelFamily != ModelFamilyCube || record.Flags != flagCubeGeometry|flagOccludesFullFace || record.FaceCoverage != 0x3f {
			t.Fatalf("construction cube has no solid terrain faces: %+v", record)
		}
	}
	wall := projection.records[2]
	connections, ok := wall.ModelState.Get(ModelStateConnections)
	if wall.ModelFamily != ModelFamilyWall || !ok || connections != 2|1<<2|1<<6|1<<8 {
		t.Fatalf("border render state = %+v", wall)
	}
	if projection.records[3].Name != retailReservedName {
		t.Fatal("construction admission widened to chemistry")
	}
}

// TestEducationCollisionUsesSharedStateFacts checks the full admitted construction palette without external fixtures.
func TestEducationCollisionUsesSharedStateFacts(t *testing.T) {
	_, records := loadV2193PhysicsInputs(t)
	count := 0
	for _, record := range records {
		if !isEducationConstructionName(record.Name) {
			continue
		}
		count++
		properties, err := sharedBlockProperties(record)
		if err != nil {
			t.Fatal(err)
		}
		seed, err := sharedCollisionSeed(record, properties)
		if err != nil {
			t.Fatal(err)
		}
		if !collisionBoxesEqual(record.CollisionSeed.Boxes, seed.Boxes) {
			t.Fatalf("construction collision differs for %s", record.Name)
		}
	}
	if count != v2193EducationStateCount {
		t.Fatalf("Education state count %d", count)
	}
}

func TestEducationConstructionRejectsUnidentifiedStateSchemas(t *testing.T) {
	for _, name := range []string{"minecraft:allow", "minecraft:deny", "minecraft:border_block"} {
		_, _, err := educationRenderRecord(Record{Name: name, StateJSON: []byte(`{"unexpected":{"type":"byte","value":1}}`)})
		if err == nil {
			t.Fatalf("unidentified state accepted for %s", name)
		}
	}
	valid := map[string]any{
		"wall_connection_type_north": map[string]any{"type": "string", "value": "none"},
		"wall_connection_type_east":  map[string]any{"type": "string", "value": "short"},
		"wall_connection_type_south": map[string]any{"type": "string", "value": "tall"},
		"wall_connection_type_west":  map[string]any{"type": "string", "value": "none"},
		"wall_post_bit":              map[string]any{"type": "int", "value": 1},
	}
	state, _ := json.Marshal(valid)
	if _, _, err := educationRenderRecord(Record{Name: "minecraft:border_block", StateJSON: state}); err == nil {
		t.Fatal("border post accepted wrong wire type")
	}
}
