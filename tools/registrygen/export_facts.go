package main

import (
	"encoding/json"
	"fmt"
	"os"

	shared "github.com/cinnabar-mc/protocolgen/generated/data"
	sharedbiome "github.com/cinnabar-mc/protocolgen/generated/data/biome"
	sharedblock "github.com/cinnabar-mc/protocolgen/generated/data/block"
)

// exportCompilerFacts emits source identities and library facts without client admission or movement policy.
func exportCompilerFacts(path string) error {
	records, err := v2193SourceRecords()
	if err != nil {
		return err
	}
	var blocks []map[string]any
	for _, r := range records {
		p, err := sharedBlockProperties(r)
		if err != nil {
			return err
		}
		boxes, ok := sharedblock.Shape(p.CollisionShape)
		if !ok {
			return fmt.Errorf("missing source collision shape for %s", r.Name)
		}
		blocks = append(blocks, blockFact(r, p, boxes))
	}
	var biomes []map[string]any
	if err := validateSharedBiomeTarget(); err != nil {
		return err
	}
	for _, b := range sharedbiome.All() {
		biomes = append(biomes, map[string]any{"id": b.ID, "has_id": b.HasID, "name": b.Name})
	}
	data, err := json.Marshal(map[string]any{"game_version": shared.MinecraftVersion, "protocol": shared.ProtocolVersion, "source_lock_sha256": shared.SourceLockSHA256, "cloudburst_ref": shared.CloudburstRef, "blocks": blocks, "biomes": biomes})
	if err != nil {
		return err
	}
	return os.WriteFile(path, data, 0644)
}

// blockFact widens float32 library values to float64 so JSON carries their exact value, not Go's shortest float32 spelling.
func blockFact(r Record, p sharedblock.Properties, boxes []sharedblock.Box) map[string]any {
	var wide [][6]float64 // nil stays nil so empty shapes keep exporting as null
	for _, b := range boxes {
		var w [6]float64
		for j, v := range b {
			w[j] = float64(v)
		}
		wide = append(wide, w)
	}
	return map[string]any{"sequential_id": r.SequentialID, "network_hash": r.NetworkHash, "name": r.Name, "state": string(r.StateJSON), "shape_id": p.CollisionShape, "boxes": wide, "friction": float64(p.Friction), "emission": p.LightEmission, "filter": p.LightDampening, "can_contain_liquid_source": p.CanContainLiquidSource}
}
