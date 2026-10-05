package main

import (
	"bytes"
	"crypto/sha256"
	"errors"
	"fmt"
	"os"
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
				// DynamicLiquidBlock final registration has
				// no version gate: still and flowing types differ.
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

// relightV2193 rewrites an existing v2193 LREG with the retail corrections applied,
// bound to the same BREG; it returns the new LREG bytes and the changed-state count.
func relightV2193(bregPath, lregPath, retailPath string) ([]byte, int, error) {
	breg, err := os.ReadFile(bregPath)
	if err != nil {
		return nil, 0, fmt.Errorf("read BREG: %w", err)
	}
	_, records, err := decodeBREGRecords(breg, v2193BlockProtocol)
	if err != nil {
		return nil, 0, err
	}
	lreg, err := os.ReadFile(lregPath)
	if err != nil {
		return nil, 0, fmt.Errorf("read LREG: %w", err)
	}
	properties, err := decodeLREGProperties(lreg, breg, v2193BlockProtocol, len(records))
	if err != nil {
		return nil, 0, err
	}
	retail, err := readPMMPLightProperties(retailPath)
	if err != nil {
		return nil, 0, err
	}
	changed, err := applyRetailLightCorrections(records, properties, retail)
	if err != nil {
		return nil, 0, err
	}
	encoded, err := encodeResolvedLightRegistryForProtocol(v2193BlockProtocol, breg, records, properties)
	if err != nil {
		return nil, 0, err
	}
	if bytes.Equal(encoded, lreg) && changed != 0 {
		return nil, 0, errors.New("relight produced no byte change")
	}
	_ = sha256.Size
	return encoded, changed, nil
}
