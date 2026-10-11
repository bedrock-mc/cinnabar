package main

import (
	"fmt"
	"math"

	shared "github.com/bedrock-mc/protocolgen/generated/data"
	sharedblock "github.com/bedrock-mc/protocolgen/generated/data/block"
	"github.com/bedrock-mc/protocolgen/generated/data/registry"
)

// sharedBlockProperties joins by state hash and checks the identifier as well.
func sharedBlockProperties(record Record) (sharedblock.Properties, error) {
	state, ok := sharedblock.StateByHash(record.NetworkHash)
	if !ok {
		return sharedblock.Properties{}, fmt.Errorf("shared catalog has no block state %#x (%s)", record.NetworkHash, record.Name)
	}
	block, ok := sharedblock.BlockAt(state.Block)
	if !ok || block.Name != record.Name {
		return sharedblock.Properties{}, fmt.Errorf("shared catalog identifier does not match %s at %#x", record.Name, record.NetworkHash)
	}
	properties, ok := sharedblock.PropertiesAt(state.Properties)
	if !ok {
		return sharedblock.Properties{}, fmt.Errorf("shared catalog has no properties for %s", record.Name)
	}
	return properties, nil
}

// validateSharedTarget prevents a catalog update from silently changing this carrier's target.
func validateSharedTarget() error {
	if registry.SourceLockSHA256 != shared.SourceLockSHA256 {
		return fmt.Errorf("shared registry and property catalog have different source locks")
	}
	if shared.MinecraftVersion != v2193GameVersion || shared.ProtocolVersion != v2193BlockProtocol || sharedblock.StateCount() != v2193BlockStateCount {
		return fmt.Errorf("shared catalog target %s/%d with %d states does not match carrier %s/%d with %d states", shared.MinecraftVersion, shared.ProtocolVersion, sharedblock.StateCount(), v2193GameVersion, v2193BlockProtocol, v2193BlockStateCount)
	}
	return nil
}

// sharedCollisionSeed projects the catalog's collision boxes into the carrier's fixed point format.
func sharedCollisionSeed(record Record, properties sharedblock.Properties) (CollisionSeed, error) {
	boxes, ok := sharedblock.Shape(properties.CollisionShape)
	if !ok {
		return CollisionSeed{}, fmt.Errorf("shared collision shape is unavailable for %s at %#x", record.Name, record.NetworkHash)
	}
	if len(boxes) > maxCollisionBoxesPerRecord {
		return CollisionSeed{}, fmt.Errorf("shared collision shape for %s exceeds carrier box limit", record.Name)
	}
	seed := CollisionSeed{ShapeID: properties.CollisionShape, Confidence: CollisionConfidenceCollisionOnly, Boxes: make([]CollisionBox, len(boxes))}
	for i, raw := range boxes {
		var fixed [6]int32
		for j, v := range raw {
			scaled := math.Round(float64(v) * collisionFixedScale)
			if math.IsNaN(scaled) || math.IsInf(scaled, 0) || scaled < collisionLocalHaloMin || scaled > collisionLocalHaloMax {
				return CollisionSeed{}, fmt.Errorf("invalid shared collision coordinate for %s", record.Name)
			}
			fixed[j] = int32(scaled)
		}
		box := CollisionBox{MinX: fixed[0], MinY: fixed[1], MinZ: fixed[2], MaxX: fixed[3], MaxY: fixed[4], MaxZ: fixed[5]}
		if err := validateCollisionBox(box); err != nil {
			return CollisionSeed{}, fmt.Errorf("shared collision for %s: %w", record.Name, err)
		}
		seed.Boxes[i] = box
	}
	return seed, nil
}

// applySharedBlockFacts replaces inherited collision and light facts while leaving client render policy and reserved states intact.
func applySharedBlockFacts(records []Record) ([]byte, error) {
	if err := validateSharedTarget(); err != nil {
		return nil, err
	}
	lights := make([]byte, len(records))
	for i := range records {
		record := &records[i]
		if record.Name == retailReservedName {
			continue
		}
		properties, err := sharedBlockProperties(*record)
		if err != nil {
			return nil, err
		}
		seed, err := sharedCollisionSeed(*record, properties)
		if err != nil {
			return nil, err
		}
		record.CollisionSeed = seed
		if properties.LightEmission > 15 || properties.LightDampening > 15 {
			return nil, fmt.Errorf("shared light value is outside nibble range for %s", record.Name)
		}
		lights[i] = properties.LightEmission | properties.LightDampening<<4
	}
	// These corrections describe the client's selected BaseGameVersion and
	// dynamic light states. They do not fill gaps in the shared catalog.
	if _, err := applyRetailLightCorrections(records, lights, nil); err != nil {
		return nil, err
	}
	return lights, nil
}
