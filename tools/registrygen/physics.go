package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"io"
	"maps"
	"math"
	"os"
	"path/filepath"
	"runtime/debug"
	"slices"
	"sort"
	"strings"

	"github.com/df-mc/dragonfly/server/world"
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

const (
	pinnedPMMPPhysicsSHA      = "c9eb2a1b7751ba874ddeb04237d2a0013121a1bf03e1d5c75a78a08bae020abd"
	pinnedPrismarineBlocksSHA = "12ff90b5094006b42d87ca7c296ed1bef0e1c2d6d67498aea85b6ece9408b494"
	pinnedPrismarineStatesSHA = "c0a94f5a32597aff028918e152c76280c1823a7840fdf73cd98d7b44814ea041"
	pinnedPrismarineShapesSHA = "72a7410456a1f5f556e8c91c07e1d1f61aea5d2fb555f2c0e33eba825247aa90"
	pinnedDragonflyVersion    = "v0.11.1-0.20260714151819-dbbd8b787946"
	pinnedDragonflyModuleSum  = "h1:Qu7Qm7iBrLQWlZtz2KdouA4agQdhybV2abSdEN5NBRY="
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

type PrismarinePhysicsFact struct {
	BoundingBox string
	StateCount  int
}

type PhysicsSourceCatalog struct {
	PMMP                      map[string]PMMPLightProperties
	Prismarine                map[string]PrismarinePhysicsFact
	DragonflyTypes            map[string][]string
	RequireProductionCoverage bool
}

type physicsBehavior uint8

const (
	behaviorBed physicsBehavior = iota + 1
	behaviorBubble
	behaviorClimbable
	behaviorCobweb
	behaviorHoney
	behaviorLava
	behaviorPowderSnow
	behaviorScaffolding
	behaviorSlime
	behaviorSoulSand
	behaviorWater
)

type reviewedPhysicsOverride struct {
	Name           string
	Behavior       physicsBehavior
	StateCount     int
	BoundingBox    string
	DragonflyTypes string
}

var reviewedPhysicsOverrides = []reviewedPhysicsOverride{
	{Name: "minecraft:bed", Behavior: behaviorBed, StateCount: 16, BoundingBox: "block", DragonflyTypes: "block.Bed,world.unknownBlock"},
	{Name: "minecraft:bubble_column", Behavior: behaviorBubble, StateCount: 2, BoundingBox: "empty", DragonflyTypes: "world.unknownBlock"},
	{Name: "minecraft:cave_vines", Behavior: behaviorClimbable, StateCount: 26, BoundingBox: "empty", DragonflyTypes: "world.unknownBlock"},
	{Name: "minecraft:cave_vines_body_with_berries", Behavior: behaviorClimbable, StateCount: 26, BoundingBox: "empty", DragonflyTypes: "world.unknownBlock"},
	{Name: "minecraft:cave_vines_head_with_berries", Behavior: behaviorClimbable, StateCount: 26, BoundingBox: "empty", DragonflyTypes: "world.unknownBlock"},
	{Name: "minecraft:flowing_lava", Behavior: behaviorLava, StateCount: 16, BoundingBox: "empty", DragonflyTypes: "block.Lava"},
	{Name: "minecraft:flowing_water", Behavior: behaviorWater, StateCount: 16, BoundingBox: "empty", DragonflyTypes: "block.Water"},
	{Name: "minecraft:honey_block", Behavior: behaviorHoney, StateCount: 1, BoundingBox: "block", DragonflyTypes: "world.unknownBlock"},
	{Name: "minecraft:ladder", Behavior: behaviorClimbable, StateCount: 6, BoundingBox: "block", DragonflyTypes: "block.Ladder,world.unknownBlock"},
	{Name: "minecraft:lava", Behavior: behaviorLava, StateCount: 16, BoundingBox: "empty", DragonflyTypes: "block.Lava"},
	{Name: "minecraft:powder_snow", Behavior: behaviorPowderSnow, StateCount: 1, BoundingBox: "empty", DragonflyTypes: "world.unknownBlock"},
	{Name: "minecraft:scaffolding", Behavior: behaviorScaffolding, StateCount: 16, BoundingBox: "block", DragonflyTypes: "world.unknownBlock"},
	{Name: "minecraft:slime", Behavior: behaviorSlime, StateCount: 1, BoundingBox: "block", DragonflyTypes: "block.Slime"},
	{Name: "minecraft:soul_sand", Behavior: behaviorSoulSand, StateCount: 1, BoundingBox: "block", DragonflyTypes: "block.SoulSand"},
	{Name: "minecraft:twisting_vines", Behavior: behaviorClimbable, StateCount: 26, BoundingBox: "empty", DragonflyTypes: "world.unknownBlock"},
	{Name: "minecraft:vine", Behavior: behaviorClimbable, StateCount: 16, BoundingBox: "empty", DragonflyTypes: "block.Vines"},
	{Name: "minecraft:water", Behavior: behaviorWater, StateCount: 16, BoundingBox: "empty", DragonflyTypes: "block.Water"},
	{Name: "minecraft:web", Behavior: behaviorCobweb, StateCount: 1, BoundingBox: "empty", DragonflyTypes: "block.Cobweb"},
	{Name: "minecraft:weeping_vines", Behavior: behaviorClimbable, StateCount: 26, BoundingBox: "empty", DragonflyTypes: "world.unknownBlock"},
}

func loadPinnedPhysicsSources(pmmpRoot, prismarineRoot string, registry world.BlockRegistry) (PhysicsSourceCatalog, error) {
	pmmpPath := filepath.Join(pmmpRoot, "block_properties_table.json")
	for _, source := range []struct {
		path, hash, label string
	}{
		{pmmpPath, pinnedPMMPPhysicsSHA, "PMMP block properties"},
		{filepath.Join(prismarineRoot, "blocks.json"), pinnedPrismarineBlocksSHA, "Prismarine block behavior"},
		{filepath.Join(prismarineRoot, "blockStates.json"), pinnedPrismarineStatesSHA, "Prismarine state order"},
		{filepath.Join(prismarineRoot, "blockCollisionShapes.json"), pinnedPrismarineShapesSHA, "Prismarine collision shapes"},
	} {
		if err := requirePinnedPhysicsFile(source.path, source.hash, source.label); err != nil {
			return PhysicsSourceCatalog{}, err
		}
	}
	if err := validateDragonflyPhysicsProvenance(); err != nil {
		return PhysicsSourceCatalog{}, err
	}
	pmmp, err := readPMMPLightProperties(pmmpPath)
	if err != nil {
		return PhysicsSourceCatalog{}, fmt.Errorf("read pinned PMMP physics: %w", err)
	}
	prismarine, err := readPrismarinePhysicsFacts(filepath.Join(prismarineRoot, "blocks.json"))
	if err != nil {
		return PhysicsSourceCatalog{}, err
	}
	dragonfly, err := collectDragonflyPhysicsTypes(registry)
	if err != nil {
		return PhysicsSourceCatalog{}, err
	}
	return PhysicsSourceCatalog{PMMP: pmmp, Prismarine: prismarine, DragonflyTypes: dragonfly, RequireProductionCoverage: true}, nil
}

func requirePinnedPhysicsFile(path, expected, label string) error {
	data, err := os.ReadFile(path)
	if err != nil {
		return fmt.Errorf("read pinned %s: %w", label, err)
	}
	if len(data) > 128<<20 {
		return fmt.Errorf("pinned %s exceeds 128 MiB", label)
	}
	actual := fmt.Sprintf("%x", sha256.Sum256(data))
	if actual != expected {
		return fmt.Errorf("pinned %s SHA-256 %s does not match %s", label, actual, expected)
	}
	return nil
}

func validateDragonflyPhysicsProvenance() error {
	info, ok := debug.ReadBuildInfo()
	if !ok {
		return fmt.Errorf("Dragonfly build provenance is unavailable")
	}
	for _, dependency := range info.Deps {
		if dependency.Path == "github.com/df-mc/dragonfly" {
			return validateDragonflyPhysicsProvenanceFields(dependency.Version, dependency.Sum, dependency.Replace != nil)
		}
	}
	return fmt.Errorf("Dragonfly dependency provenance is missing")
}

func validateDragonflyPhysicsProvenanceFields(version, sum string, replaced bool) error {
	if replaced {
		return fmt.Errorf("Dragonfly dependency provenance uses an unpinned replacement")
	}
	if version != pinnedDragonflyVersion {
		return fmt.Errorf("Dragonfly version %q does not match pinned %q", version, pinnedDragonflyVersion)
	}
	if sum != pinnedDragonflyModuleSum {
		return fmt.Errorf("Dragonfly module sum %q does not match pinned %q", sum, pinnedDragonflyModuleSum)
	}
	return nil
}

func readPrismarinePhysicsFacts(path string) (map[string]PrismarinePhysicsFact, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read Prismarine behavior facts: %w", err)
	}
	var raw []struct {
		Name        string `json:"name"`
		BoundingBox string `json:"boundingBox"`
		MinStateID  int    `json:"minStateId"`
		MaxStateID  int    `json:"maxStateId"`
	}
	if err := json.Unmarshal(data, &raw); err != nil {
		return nil, fmt.Errorf("decode Prismarine behavior facts: %w", err)
	}
	facts := make(map[string]PrismarinePhysicsFact, len(raw))
	for _, block := range raw {
		if block.Name == "" || (block.BoundingBox != "empty" && block.BoundingBox != "block") || block.MinStateID < 0 || block.MaxStateID < block.MinStateID {
			return nil, fmt.Errorf("invalid Prismarine behavior fact for %q", block.Name)
		}
		if _, exists := facts[block.Name]; exists {
			return nil, fmt.Errorf("duplicate Prismarine behavior fact %q", block.Name)
		}
		facts[block.Name] = PrismarinePhysicsFact{BoundingBox: block.BoundingBox, StateCount: block.MaxStateID - block.MinStateID + 1}
	}
	return facts, nil
}

func collectDragonflyPhysicsTypes(registry world.BlockRegistry) (map[string][]string, error) {
	if registry == nil {
		return nil, fmt.Errorf("Dragonfly physics registry is nil")
	}
	registry.Finalize()
	typeSets := make(map[string]map[string]struct{})
	for _, value := range registry.Blocks() {
		name, _ := value.EncodeBlock()
		name = canonicalBlockName(name)
		if typeSets[name] == nil {
			typeSets[name] = make(map[string]struct{})
		}
		typeSets[name][fmt.Sprintf("%T", value)] = struct{}{}
	}
	types := make(map[string][]string, len(typeSets))
	for name, set := range typeSets {
		types[name] = slices.Sorted(maps.Keys(set))
	}
	return types, nil
}

func buildPhysicsRecords(records []Record, sources PhysicsSourceCatalog) ([]PhysicsRecord, error) {
	if err := validateReviewedPhysicsOverrides(); err != nil {
		return nil, err
	}
	counts := make(map[string]int)
	for _, record := range records {
		counts[record.Name]++
	}
	if err := crossCheckReviewedPhysicsOverrides(counts, sources); err != nil {
		return nil, err
	}
	physics := make([]PhysicsRecord, len(records))
	for i, record := range records {
		properties, ok := sources.PMMP[record.Name]
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
		if override, ok := reviewedPhysicsOverrideFor(record.Name); ok {
			if err := applyPhysicsOverride(record, override, &entry); err != nil {
				return nil, fmt.Errorf("physics facts for %q: %w", record.Name, err)
			}
		}
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

func validateReviewedPhysicsOverrides() error {
	for index, override := range reviewedPhysicsOverrides {
		if override.Name == "" || override.StateCount <= 0 || override.DragonflyTypes == "" || (override.BoundingBox != "empty" && override.BoundingBox != "block") {
			return fmt.Errorf("invalid reviewed physics override at index %d", index)
		}
		if index > 0 && reviewedPhysicsOverrides[index-1].Name >= override.Name {
			return fmt.Errorf("reviewed physics overrides are not strictly sorted at %q", override.Name)
		}
	}
	return nil
}

func crossCheckReviewedPhysicsOverrides(counts map[string]int, sources PhysicsSourceCatalog) error {
	for _, override := range reviewedPhysicsOverrides {
		count := counts[override.Name]
		if count == 0 {
			if sources.RequireProductionCoverage {
				return fmt.Errorf("reviewed physics override %q has no canonical states", override.Name)
			}
			continue
		}
		if sources.RequireProductionCoverage && count != override.StateCount {
			return fmt.Errorf("reviewed physics override %q has %d states, expected %d", override.Name, count, override.StateCount)
		}
		prismarine, ok := sources.Prismarine[strings.TrimPrefix(override.Name, "minecraft:")]
		if !ok {
			return fmt.Errorf("Prismarine behavior source is missing %q", override.Name)
		}
		if prismarine.BoundingBox != override.BoundingBox {
			return fmt.Errorf("Prismarine behavior for %q is %q, expected %q", override.Name, prismarine.BoundingBox, override.BoundingBox)
		}
		if sources.RequireProductionCoverage && prismarine.StateCount != override.StateCount {
			return fmt.Errorf("Prismarine state count for %q is %d, expected %d", override.Name, prismarine.StateCount, override.StateCount)
		}
		dragonflyTypes, ok := sources.DragonflyTypes[override.Name]
		if !ok {
			return fmt.Errorf("Dragonfly behavior source is missing %q", override.Name)
		}
		if actual := strings.Join(dragonflyTypes, ","); actual != override.DragonflyTypes {
			return fmt.Errorf("Dragonfly behavior types for %q are %q, expected %q", override.Name, actual, override.DragonflyTypes)
		}
	}
	return nil
}

func reviewedPhysicsOverrideFor(name string) (reviewedPhysicsOverride, bool) {
	index := sort.Search(len(reviewedPhysicsOverrides), func(index int) bool {
		return reviewedPhysicsOverrides[index].Name >= name
	})
	if index == len(reviewedPhysicsOverrides) || reviewedPhysicsOverrides[index].Name != name {
		return reviewedPhysicsOverride{}, false
	}
	return reviewedPhysicsOverrides[index], true
}

func applyPhysicsOverride(record Record, override reviewedPhysicsOverride, entry *PhysicsRecord) error {
	switch override.Behavior {
	case behaviorWater:
		entry.Flags |= physicsFlagWater | physicsFlagPassable
		entry.Boxes = nil
		entry.FluidHeightQ1E8 = liquidHeight(record)
	case behaviorLava:
		entry.Flags |= physicsFlagLava | physicsFlagPassable
		entry.Boxes = nil
		entry.FluidHeightQ1E8 = liquidHeight(record)
	case behaviorClimbable:
		entry.Flags |= physicsFlagClimbable
	case behaviorCobweb:
		entry.Flags |= physicsFlagCobweb | physicsFlagPassable
		entry.HorizontalSpeedQ1E8 = 25_000_000
		entry.VerticalSpeedQ1E8 = 5_000_000
	case behaviorPowderSnow:
		entry.Flags |= physicsFlagPowderSnow | physicsFlagPassable
	case behaviorScaffolding:
		entry.Flags |= physicsFlagScaffolding | physicsFlagClimbable
	case behaviorSlime:
		entry.SurfaceResponse = SurfaceSlime
	case behaviorHoney:
		entry.HorizontalSpeedQ1E8 = 40_000_000
		entry.SurfaceResponse = SurfaceHoney
	case behaviorSoulSand:
		entry.HorizontalSpeedQ1E8 = 40_000_000
		entry.SurfaceResponse = SurfaceSoulSand
	case behaviorBubble:
		dragDown, err := strictBubbleDragDown(record.StateJSON)
		if err != nil {
			return err
		}
		entry.Flags |= physicsFlagWater | physicsFlagPassable
		entry.Boxes = nil
		entry.FluidHeightQ1E8 = defaultSpeedQ1E8
		if dragDown {
			entry.SurfaceResponse = SurfaceBubbleDown
		} else {
			entry.SurfaceResponse = SurfaceBubbleUp
		}
	case behaviorBed:
		entry.SurfaceResponse = SurfaceBed
	default:
		return fmt.Errorf("unknown reviewed behavior %d", override.Behavior)
	}
	return nil
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

func strictBubbleDragDown(state []byte) (bool, error) {
	decoder := json.NewDecoder(bytes.NewReader(state))
	token, err := decoder.Token()
	if err != nil || token != json.Delim('{') {
		return false, fmt.Errorf("drag_down state is malformed")
	}
	seen := make(map[string]struct{})
	var dragDown *bool
	for decoder.More() {
		keyToken, err := decoder.Token()
		if err != nil {
			return false, fmt.Errorf("drag_down state key is malformed")
		}
		key, ok := keyToken.(string)
		if !ok {
			return false, fmt.Errorf("drag_down state key is not a string")
		}
		if _, duplicate := seen[key]; duplicate {
			return false, fmt.Errorf("drag_down state contains duplicate fact %q", key)
		}
		seen[key] = struct{}{}
		var scalar struct {
			Type  string          `json:"type"`
			Value json.RawMessage `json:"value"`
		}
		if err := decoder.Decode(&scalar); err != nil {
			return false, fmt.Errorf("drag_down fact is malformed: %w", err)
		}
		if key != "drag_down" {
			return false, fmt.Errorf("drag_down state contains unknown fact %q", key)
		}
		if scalar.Type != "byte" {
			return false, fmt.Errorf("drag_down fact has unknown type %q", scalar.Type)
		}
		var value int
		if err := json.Unmarshal(scalar.Value, &value); err != nil || (value != 0 && value != 1) {
			return false, fmt.Errorf("drag_down fact must be byte 0 or 1")
		}
		resolved := value == 1
		dragDown = &resolved
	}
	if _, err := decoder.Token(); err != nil {
		return false, fmt.Errorf("drag_down state object is unterminated")
	}
	var trailing any
	if err := decoder.Decode(&trailing); err != io.EOF {
		return false, fmt.Errorf("drag_down state has trailing values")
	}
	if dragDown == nil {
		return false, fmt.Errorf("drag_down fact is missing")
	}
	return *dragDown, nil
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
