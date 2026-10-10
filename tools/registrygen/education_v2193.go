package main

import (
	"bytes"
	"encoding/json"
	"fmt"
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
