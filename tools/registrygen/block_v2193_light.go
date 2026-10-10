package main

import (
	"errors"
)

// Dragonfly reports (emission 0, filter 15) for a state it has no block implementation
// for, so unimplemented translucent plants (glow lichen, cave vines, hanging roots, ...)
// inherited a full light filter. The pinned retail block table carries their real values.
const (
	unknownBlockEmission = 0
	unknownBlockFilter   = 15
)

// applyRetailLightCorrections applies identified native corrections, then replaces
// unimplemented-block defaults in properties (emission | filter<<4, parallel to
// records) with retail values. It returns the number of changed states.
func applyRetailLightCorrections(records []Record, properties []byte, retail map[string]PMMPLightProperties) (int, error) {
	if len(records) != len(properties) {
		return 0, errors.New("light property count does not match records")
	}
	changed := 0
	for index, record := range records {
		if record.Name == retailReservedName {
			continue
		}
		current := properties[index]
		// Every portal axis emits eleven and transmits light, including the
		// unknown-axis state absent from Dragonfly's block implementation.
		if record.Name == "minecraft:portal" {
			const nativePortalLight byte = 11
			if current != nativePortalLight {
				properties[index] = nativePortalLight
				changed++
			}
			continue
		}
		// Snow layers transmit light regardless of their height. Ice uses
		// dampening three; packed and blue ice retain their separate rules.
		isSnowLayer := record.Name == "minecraft:snow_layer"
		isTransparentIce := record.Name == "minecraft:ice" || record.Name == "minecraft:frosted_ice"
		isStillWater := record.Name == "minecraft:water"
		isFlowingWater := record.Name == "minecraft:flowing_water"
		if isSnowLayer || isTransparentIce || isStillWater || isFlowingWater {
			var filter byte
			switch {
			case isTransparentIce:
				filter = 3
			case isStillWater:
				// Final registration uses one for a
				// current BaseGameVersion (>= native gate 1.21.130).
				// Older level compatibility retains two; this projection
				// targets the current registry-foundation game version.
				filter = 1
			case isFlowingWater:
				// Vanilla flowing liquid has no version gate here:
				// still and flowing types differ.
				filter = 2
			}
			next := current&0x0f | filter<<4
			if next != current {
				properties[index] = next
				changed++
			}
			continue
		}
		emission, stateResolved, err := stateEmission(record)
		if err != nil {
			return 0, err
		}
		if stateResolved {
			next := current&0xf0 | emission
			if record.Name == "minecraft:sculk_sensor" || record.Name == "minecraft:calibrated_sculk_sensor" {
				if exact, ok := retail[record.Name]; ok {
					_, filter, err := checkedPMMPLight(record.Name, exact)
					if err != nil {
						return 0, err
					}
					next = filter<<4 | emission
				}
			}
			if next != current {
				properties[index] = next
				changed++
			}
			continue
		}
		if current&0x0f != unknownBlockEmission || current>>4 != unknownBlockFilter {
			continue
		}
		exact, ok := retail[record.Name]
		if !ok {
			continue
		}
		emission, filter, err := checkedPMMPLight(record.Name, exact)
		if err != nil {
			return 0, err
		}
		next := emission | filter<<4
		if next != current {
			properties[index] = next
			changed++
		}
	}
	return changed, nil
}
