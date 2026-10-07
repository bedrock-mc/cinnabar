package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"path/filepath"
)

const v2193EducationStateCount = 2 + 3*3*3*3*2

// Education construction blocks have terrain renderers even when their items
// are absent from the ordinary creative-item catalog.
func educationRenderRecord(identity Record) (Record, bool, error) {
	if !isEducationConstructionName(identity.Name) {
		return Record{}, false, nil
	}
	var state map[string]struct {
		Type  string          `json:"type"`
		Value json.RawMessage `json:"value"`
	}
	decoder := json.NewDecoder(bytes.NewReader(identity.StateJSON))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&state); err != nil {
		return Record{}, false, fmt.Errorf("Education construction state: %w", err)
	}
	record := identity
	record.Flags, record.FaceCoverage = 0, 0
	record.ContributorRole = ContributorPrimary
	record.ModelState = ModelState{}
	if identity.Name != "minecraft:border_block" {
		if len(state) != 0 {
			return Record{}, false, fmt.Errorf("Education cube %s has unexpected state properties", identity.Name)
		}
		record.Flags = flagCubeGeometry | flagOccludesFullFace
		record.ModelFamily = ModelFamilyCube
		record.FaceCoverage = 0x3f
		return record, true, nil
	}
	if len(state) != 5 {
		return Record{}, false, fmt.Errorf("Education border has unexpected state properties")
	}
	var connections uint32
	for index, direction := range []string{"north", "east", "south", "west"} {
		property, ok := state["wall_connection_type_"+direction]
		var value string
		if !ok || property.Type != "string" || json.Unmarshal(property.Value, &value) != nil {
			return Record{}, false, fmt.Errorf("Education border has invalid %s connection", direction)
		}
		var selected uint32
		switch value {
		case "none":
		case "short":
			selected = 1
		case "tall":
			selected = 2
		default:
			return Record{}, false, fmt.Errorf("Education border has unknown connection %q", value)
		}
		connections |= selected << (2 * index)
	}
	post := state["wall_post_bit"]
	var value uint32
	if post.Type != "byte" || json.Unmarshal(post.Value, &value) != nil || value > 1 {
		return Record{}, false, fmt.Errorf("Education border has invalid post bit")
	}
	connections |= value << 8
	record.ModelFamily = ModelFamilyWall
	record.ModelState.Set(ModelStateConnections, connections)
	return record, true, nil
}

// isEducationConstructionName limits admission to the three construction blocks.
func isEducationConstructionName(name string) bool {
	return name == "minecraft:allow" || name == "minecraft:deny" || name == "minecraft:border_block"
}

// applyEducationCollisionSeeds joins construction states to exact pinned collision facts.
func applyEducationCollisionSeeds(records []Record, root string) error {
	statesPath, shapesPath := filepath.Join(root, "blockStates.json"), filepath.Join(root, "blockCollisionShapes.json")
	if err := requirePinnedPhysicsFile(statesPath, pinnedPrismarineStatesSHA, "Education state order"); err != nil {
		return err
	}
	if err := requirePinnedPhysicsFile(shapesPath, pinnedPrismarineShapesSHA, "Education collision shapes"); err != nil {
		return err
	}
	states, err := readPrismarineStates(statesPath, shapesPath)
	if err != nil {
		return err
	}
	seeds := make(map[string]CollisionSeed)
	for _, state := range states {
		if !isEducationConstructionName(state.Name) {
			continue
		}
		canonical, err := canonicalTypedState(state.Properties)
		if err != nil {
			return err
		}
		key := canonicalRecordKey(state.Name, canonical)
		if _, duplicate := seeds[key]; duplicate {
			return fmt.Errorf("duplicate Education collision source %s", key)
		}
		seeds[key] = state.CollisionSeed
	}
	if len(seeds) != v2193EducationStateCount {
		return fmt.Errorf("Education collision source has %d states, want %d", len(seeds), v2193EducationStateCount)
	}
	matched := 0
	for index, record := range records {
		if !isEducationConstructionName(record.Name) {
			continue
		}
		seed, ok := seeds[canonicalRecordKey(record.Name, record.StateJSON)]
		if !ok {
			return fmt.Errorf("Education palette state has no exact collision source: %s", record.Name)
		}
		records[index].CollisionSeed = seed
		records[index].Provenance |= ProvenancePrismarine
		matched++
	}
	if matched != len(seeds) {
		return fmt.Errorf("Education palette matched %d collision states, want %d", matched, len(seeds))
	}
	return nil
}

// applyEducationLightProperties uses identified light facts for admitted construction states.
func applyEducationLightProperties(projection v2193Projection, properties []byte, source map[string]PMMPLightProperties) error {
	for index, record := range projection.records {
		if projection.classes[index] != v2193ClassEducation {
			continue
		}
		fact, ok := source[record.Name]
		if !ok {
			return fmt.Errorf("Education palette has no identified light properties for %s", record.Name)
		}
		emission, filter, err := checkedPMMPLight(record.Name, fact)
		if err != nil {
			return err
		}
		properties[index] = emission | filter<<4
	}
	return nil
}
