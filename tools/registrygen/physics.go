package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"math"
	"sort"
	"strings"
)

const (
	physicsRegistryHeader = "PREG1001"
	physicsRecordCount    = 16_913
	maxPhysicsBoxes       = 32
	defaultFrictionQ1E8   = 60_000_000
	defaultSpeedQ1E8      = 100_000_000
)

const (
	physicsFlagClimbable uint8 = 1 << iota
	physicsFlagWater
	physicsFlagLava
	physicsFlagCobweb
	physicsFlagPowderSnow
	physicsFlagScaffolding
	physicsFlagPassable
	physicsFlagReserved
)

type SurfaceResponse uint8

const (
	SurfaceNone SurfaceResponse = iota
	SurfaceSlime
	SurfaceBed
	SurfaceHoney
	SurfaceSoulSand
	SurfaceBubbleUp
	SurfaceBubbleDown
)

type PhysicsRecord struct {
	SequentialID        uint32
	NetworkHash         uint32
	Boxes               []CollisionBox
	FrictionQ1E8        uint32
	HorizontalSpeedQ1E8 uint32
	VerticalSpeedQ1E8   uint32
	FluidHeightQ1E8     int32
	Flags               uint8
	SurfaceResponse     SurfaceResponse
}

func buildPhysicsRecords(records []Record, pmmp map[string]PMMPLightProperties) ([]PhysicsRecord, error) {
	physics := make([]PhysicsRecord, len(records))
	for i, record := range records {
		properties, ok := pmmp[record.Name]
		if !ok {
			return nil, fmt.Errorf("PMMP physics properties are missing canonical block %q", record.Name)
		}
		friction, err := fixedPhysicsScalar(properties.Friction)
		if err != nil {
			return nil, fmt.Errorf("PMMP friction for %q: %w", record.Name, err)
		}
		entry := PhysicsRecord{
			SequentialID:        record.SequentialID,
			NetworkHash:         record.NetworkHash,
			Boxes:               append([]CollisionBox(nil), record.CollisionSeed.Boxes...),
			FrictionQ1E8:        friction,
			HorizontalSpeedQ1E8: defaultSpeedQ1E8,
			VerticalSpeedQ1E8:   defaultSpeedQ1E8,
		}
		if len(entry.Boxes) == 0 {
			entry.Flags |= physicsFlagPassable
		}
		applyPhysicsFacts(record, &entry)
		physics[i] = entry
	}
	return physics, nil
}

func fixedPhysicsScalar(value float64) (uint32, error) {
	if math.IsNaN(value) || math.IsInf(value, 0) || value <= 0 {
		return 0, fmt.Errorf("invalid scalar %v", value)
	}
	// PMMP's JSON preserves binary-float noise (for example
	// 0.6000000238418579). Normalize to six reviewed decimal places before
	// encoding the required 1e-8 fixed-point representation.
	normalized := math.Round(value*1_000_000) / 1_000_000
	scaled := math.Round(normalized * 100_000_000)
	if scaled <= 0 || scaled > math.MaxUint32 {
		return 0, fmt.Errorf("scalar %v is outside Q1e8 bounds", value)
	}
	return uint32(scaled), nil
}

func applyPhysicsFacts(record Record, entry *PhysicsRecord) {
	name := strings.TrimPrefix(record.Name, "minecraft:")
	switch name {
	case "water", "flowing_water":
		entry.Flags |= physicsFlagWater | physicsFlagPassable
		entry.Boxes = nil
		entry.FluidHeightQ1E8 = liquidHeight(record)
	case "lava", "flowing_lava":
		entry.Flags |= physicsFlagLava | physicsFlagPassable
		entry.Boxes = nil
		entry.FluidHeightQ1E8 = liquidHeight(record)
	case "ladder", "vine", "weeping_vines", "weeping_vines_plant", "twisting_vines", "twisting_vines_plant":
		entry.Flags |= physicsFlagClimbable
	case "web", "cobweb":
		entry.Flags |= physicsFlagCobweb | physicsFlagPassable
		entry.HorizontalSpeedQ1E8 = 25_000_000
		entry.VerticalSpeedQ1E8 = 5_000_000
	case "powder_snow":
		entry.Flags |= physicsFlagPowderSnow | physicsFlagPassable
	case "scaffolding":
		entry.Flags |= physicsFlagScaffolding | physicsFlagClimbable
	case "slime", "slime_block":
		entry.SurfaceResponse = SurfaceSlime
	case "honey_block":
		entry.HorizontalSpeedQ1E8 = 40_000_000
		entry.SurfaceResponse = SurfaceHoney
	case "soul_sand":
		entry.HorizontalSpeedQ1E8 = 40_000_000
		entry.SurfaceResponse = SurfaceSoulSand
	case "bubble_column":
		entry.Flags |= physicsFlagWater | physicsFlagPassable
		entry.Boxes = nil
		entry.FluidHeightQ1E8 = defaultSpeedQ1E8
		if stateBool(record.StateJSON, "drag_down") {
			entry.SurfaceResponse = SurfaceBubbleDown
		} else {
			entry.SurfaceResponse = SurfaceBubbleUp
		}
	default:
		if strings.HasSuffix(name, "_bed") || name == "bed" {
			entry.SurfaceResponse = SurfaceBed
		}
	}
}

func liquidHeight(record Record) int32 {
	depth, ok := record.ModelState.Get(ModelStateLiquidDepth)
	if !ok {
		return defaultSpeedQ1E8
	}
	if depth >= 8 {
		return defaultSpeedQ1E8
	}
	return int32((8 - depth) * defaultSpeedQ1E8 / 9)
}

func stateBool(state []byte, key string) bool {
	var values map[string]struct {
		Type  string          `json:"type"`
		Value json.RawMessage `json:"value"`
	}
	if json.Unmarshal(state, &values) != nil {
		return false
	}
	scalar, ok := values[key]
	if !ok {
		return false
	}
	switch scalar.Type {
	case "byte", "int":
		var value int64
		return json.Unmarshal(scalar.Value, &value) == nil && value != 0
	case "string":
		var value string
		return json.Unmarshal(scalar.Value, &value) == nil && (value == "true" || value == "1")
	}
	return false
}

func encodePhysicsRegistry(breg []byte, records []PhysicsRecord, expectedCount int) ([]byte, error) {
	if len(records) != expectedCount {
		return nil, fmt.Errorf("physics record count %d does not match expected %d", len(records), expectedCount)
	}
	if expectedCount < 0 || expectedCount > maxRecordCount {
		return nil, fmt.Errorf("physics record count %d is outside bounds", expectedCount)
	}
	sorted := append([]PhysicsRecord(nil), records...)
	sort.Slice(sorted, func(i, j int) bool { return sorted[i].SequentialID < sorted[j].SequentialID })
	seenHashes := make(map[uint32]struct{}, len(sorted))
	encoded := make([]byte, 0, 48+len(sorted)*36+32)
	encoded = append(encoded, physicsRegistryHeader...)
	encoded = binary.LittleEndian.AppendUint32(encoded, registryProtocol)
	encoded = binary.LittleEndian.AppendUint32(encoded, uint32(len(sorted)))
	bregDigest := sha256.Sum256(breg)
	encoded = append(encoded, bregDigest[:]...)
	for index, record := range sorted {
		if record.SequentialID != uint32(index) {
			return nil, fmt.Errorf("physics sequential ID %d is not contiguous index %d", record.SequentialID, index)
		}
		if _, exists := seenHashes[record.NetworkHash]; exists {
			return nil, fmt.Errorf("duplicate physics network hash %#x", record.NetworkHash)
		}
		seenHashes[record.NetworkHash] = struct{}{}
		if err := validatePhysicsRecord(record); err != nil {
			return nil, fmt.Errorf("physics sequential ID %d: %w", record.SequentialID, err)
		}
		encoded = binary.LittleEndian.AppendUint32(encoded, record.SequentialID)
		encoded = binary.LittleEndian.AppendUint32(encoded, record.NetworkHash)
		encoded = append(encoded, byte(len(record.Boxes)), record.Flags, byte(record.SurfaceResponse), 0)
		encoded = binary.LittleEndian.AppendUint32(encoded, record.FrictionQ1E8)
		encoded = binary.LittleEndian.AppendUint32(encoded, record.HorizontalSpeedQ1E8)
		encoded = binary.LittleEndian.AppendUint32(encoded, record.VerticalSpeedQ1E8)
		encoded = binary.LittleEndian.AppendUint32(encoded, uint32(record.FluidHeightQ1E8))
		for _, box := range record.Boxes {
			for _, value := range [...]int32{box.MinX, box.MinY, box.MinZ, box.MaxX, box.MaxY, box.MaxZ} {
				encoded = binary.LittleEndian.AppendUint32(encoded, uint32(value))
			}
		}
	}
	digest := sha256.Sum256(encoded)
	return append(encoded, digest[:]...), nil
}

func validatePhysicsRecord(record PhysicsRecord) error {
	if len(record.Boxes) > maxPhysicsBoxes {
		return fmt.Errorf("box count %d exceeds %d", len(record.Boxes), maxPhysicsBoxes)
	}
	if record.Flags&physicsFlagReserved != 0 {
		return fmt.Errorf("unknown flags %#x", record.Flags)
	}
	if record.Flags&physicsFlagWater != 0 && record.Flags&physicsFlagLava != 0 {
		return fmt.Errorf("water and lava flags conflict")
	}
	if record.FrictionQ1E8 == 0 || record.HorizontalSpeedQ1E8 == 0 || record.VerticalSpeedQ1E8 == 0 {
		return fmt.Errorf("friction and speed factors must be non-zero")
	}
	if record.SurfaceResponse > SurfaceBubbleDown {
		return fmt.Errorf("unknown surface response %d", record.SurfaceResponse)
	}
	if (record.SurfaceResponse == SurfaceBubbleUp || record.SurfaceResponse == SurfaceBubbleDown) && record.Flags&physicsFlagWater == 0 {
		return fmt.Errorf("bubble response requires water")
	}
	fluid := record.Flags&(physicsFlagWater|physicsFlagLava) != 0
	if fluid != (record.FluidHeightQ1E8 > 0) || record.FluidHeightQ1E8 > defaultSpeedQ1E8 {
		return fmt.Errorf("fluid height does not match fluid flags")
	}
	if fluid && record.Flags&physicsFlagPassable != 0 && len(record.Boxes) != 0 {
		return fmt.Errorf("passable fluid has collision boxes")
	}
	for _, box := range record.Boxes {
		if err := validateCollisionBox(box); err != nil {
			return fmt.Errorf("invalid box: %w", err)
		}
	}
	return nil
}

func validatePhysicsBindingBREG(generated, binding []byte, records []Record) error {
	if !bytes.Equal(generated, binding) {
		return fmt.Errorf("physics-binding BREG is not byte-identical to generated BREG1003")
	}
	return validateLightBindingBREG(binding, records)
}
