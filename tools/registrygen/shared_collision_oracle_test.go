package main

import (
	"fmt"
	"math"

	sharedblock "github.com/cinnabar-mc/protocolgen/generated/data/block"
)

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
