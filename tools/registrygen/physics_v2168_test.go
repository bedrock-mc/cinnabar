package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"strings"
	"testing"
)

func loadV2168PhysicsInputs(t *testing.T) ([]byte, []Record) {
	t.Helper()
	path := filepath.Join("..", "..", "crates", "assets", "data", "block-registry-v2168.bin")
	breg, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if got := hexDigest(breg); got != v2168PhysicsBREGSHA256 {
		t.Fatalf("v2168 BREG SHA-256 = %s, want %s", got, v2168PhysicsBREGSHA256)
	}
	_, records, err := decodeBREGRecords(breg, v2168BlockProtocol)
	if err != nil {
		t.Fatal(err)
	}
	return breg, records
}

func v2168RecordsNamed(records []Record, name string) []int {
	var indexes []int
	for index, record := range records {
		if record.Name == name {
			indexes = append(indexes, index)
		}
	}
	return indexes
}

type v2168PhysicsEntry struct {
	SequentialID    uint32
	NetworkHash     uint32
	Boxes           []CollisionBox
	Flags           uint8
	Surface         SurfaceResponse
	Friction        uint32
	HorizontalSpeed uint32
	VerticalSpeed   uint32
	FluidHeight     uint32
}

// decodeV2168PhysicsArtifact performs the exact structural walk the production
// decoder owes the artifact: header identity, pinned-BREG digest binding,
// contiguous identities, bounded payload accounting, and trailer digest.
func decodeV2168PhysicsArtifact(t *testing.T, artifact, breg []byte, count int) []v2168PhysicsEntry {
	t.Helper()
	if len(artifact) < 48+32 {
		t.Fatal("physics artifact is truncated")
	}
	if string(artifact[:8]) != physicsRegistryHeader {
		t.Fatalf("magic = %q", string(artifact[:8]))
	}
	if got := binary.LittleEndian.Uint32(artifact[8:12]); got != v2168BlockProtocol {
		t.Fatalf("protocol = %d, want %d", got, v2168BlockProtocol)
	}
	if got := binary.LittleEndian.Uint32(artifact[12:16]); got != uint32(count) {
		t.Fatalf("record count = %d, want %d", got, count)
	}
	bregDigest := sha256.Sum256(breg)
	if !bytes.Equal(artifact[16:48], bregDigest[:]) {
		t.Fatal("physics artifact does not bind the exact pinned v2168 BREG digest")
	}
	cursor := 48
	entries := make([]v2168PhysicsEntry, 0, count)
	end := len(artifact) - 32
	for index := 0; index < count; index++ {
		if cursor+28 > end {
			t.Fatalf("physics record %d is truncated", index)
		}
		prefix := artifact[cursor : cursor+28]
		entry := v2168PhysicsEntry{
			SequentialID:    binary.LittleEndian.Uint32(prefix[0:4]),
			NetworkHash:     binary.LittleEndian.Uint32(prefix[4:8]),
			Flags:           prefix[9],
			Surface:         SurfaceResponse(prefix[10]),
			Friction:        binary.LittleEndian.Uint32(prefix[12:16]),
			HorizontalSpeed: binary.LittleEndian.Uint32(prefix[16:20]),
			VerticalSpeed:   binary.LittleEndian.Uint32(prefix[20:24]),
			FluidHeight:     binary.LittleEndian.Uint32(prefix[24:28]),
		}
		if prefix[11] != 0 {
			t.Fatalf("physics record %d has a nonzero reserved byte", index)
		}
		boxCount := int(int8(prefix[8]))
		if boxCount < 0 || boxCount > maxPhysicsBoxes {
			t.Fatalf("physics record %d declares %d boxes", index, boxCount)
		}
		boxStart := cursor + 28
		if boxStart+boxCount*24 > end {
			t.Fatalf("physics record %d boxes are truncated", index)
		}
		for boxIndex := 0; boxIndex < boxCount; boxIndex++ {
			box := artifact[boxStart+boxIndex*24 : boxStart+(boxIndex+1)*24]
			entry.Boxes = append(entry.Boxes, CollisionBox{
				MinX: int32(binary.LittleEndian.Uint32(box[0:4])),
				MinY: int32(binary.LittleEndian.Uint32(box[4:8])),
				MinZ: int32(binary.LittleEndian.Uint32(box[8:12])),
				MaxX: int32(binary.LittleEndian.Uint32(box[12:16])),
				MaxY: int32(binary.LittleEndian.Uint32(box[16:20])),
				MaxZ: int32(binary.LittleEndian.Uint32(box[20:24])),
			})
		}
		if entry.SequentialID != uint32(index) {
			t.Fatalf("physics record %d carries sequential ID %d", index, entry.SequentialID)
		}
		entries = append(entries, entry)
		cursor = boxStart + boxCount*24
	}
	if cursor != end {
		t.Fatalf("physics payload ends at %d, want %d", cursor, end)
	}
	payloadDigest := sha256.Sum256(artifact[:end])
	if !bytes.Equal(artifact[end:], payloadDigest[:]) {
		t.Fatal("physics artifact trailer digest mismatch")
	}
	return entries
}

func v2168PhysicsEntryIsNeutralReserved(entry v2168PhysicsEntry) bool {
	return entry.Flags == physicsFlagPassable && len(entry.Boxes) == 0 &&
		entry.Surface == SurfaceNone && entry.FluidHeight == 0 &&
		entry.Friction == defaultSpeedQ1E8 && entry.HorizontalSpeed == defaultSpeedQ1E8 &&
		entry.VerticalSpeed == defaultSpeedQ1E8
}

func TestV2168PhysicsArtifactBindsPinnedBREGWith969NeutralReservedAndExactFacts(t *testing.T) {
	breg, records := loadV2168PhysicsInputs(t)
	dataDir := filepath.Join("..", "..", "crates", "assets", "data")
	artifact, err := os.ReadFile(filepath.Join(dataDir, "block-physics-v2168.bin"))
	if err != nil {
		t.Fatal(err)
	}
	shaBytes, err := os.ReadFile(filepath.Join(dataDir, "block-physics-v2168.sha256"))
	if err != nil {
		t.Fatal(err)
	}
	// Mirrors the protocol-1001 checksum pattern: bare lowercase hex digest.
	if strings.TrimSpace(string(shaBytes)) != hexDigest(artifact) {
		t.Fatal("checked-in v2168 physics checksum does not match the artifact")
	}
	entries := decodeV2168PhysicsArtifact(t, artifact, breg, v2168BlockStateCount)

	seenHashes := make(map[uint32]struct{}, len(entries))
	reserved := 0
	for index, entry := range entries {
		if _, duplicate := seenHashes[entry.NetworkHash]; duplicate {
			t.Fatalf("runtime ID %d repeats network hash %#x", index, entry.NetworkHash)
		}
		seenHashes[entry.NetworkHash] = struct{}{}
		if records[index].Name != retailReservedName {
			continue
		}
		reserved++
		if !v2168PhysicsEntryIsNeutralReserved(entry) {
			t.Fatalf("reserved runtime ID %d is not exactly neutral: %+v", index, entry)
		}
	}
	if reserved != v2168PhysicsReservedCount {
		t.Fatalf("neutral reserved records = %d, want exactly %d", reserved, v2168PhysicsReservedCount)
	}

	fluidHeights := func(name string, wantStates int, fluidBit uint8) map[uint32]int {
		t.Helper()
		indexes := v2168RecordsNamed(records, name)
		if len(indexes) != wantStates {
			t.Fatalf("%s state count = %d, want %d", name, len(indexes), wantStates)
		}
		heights := make(map[uint32]int)
		for _, index := range indexes {
			entry := entries[index]
			if entry.Flags&(fluidBit|physicsFlagPassable) != fluidBit|physicsFlagPassable ||
				len(entry.Boxes) != 0 || entry.FluidHeight == 0 {
				t.Fatalf("%s runtime ID %d lost its fluid facts: %+v", name, index, entry)
			}
			heights[entry.FluidHeight]++
		}
		return heights
	}
	waterHeights := fluidHeights("minecraft:water", 16, physicsFlagWater)
	for _, want := range []uint32{100_000_000, 88_888_888, 55_555_555, 11_111_111} {
		if _, ok := waterHeights[want]; !ok {
			t.Fatalf("water fluid heights miss %d: %v", want, waterHeights)
		}
	}
	lavaHeights := fluidHeights("minecraft:lava", 16, physicsFlagLava)
	for _, want := range []uint32{100_000_000, 88_888_888, 55_555_555, 11_111_111} {
		if _, ok := lavaHeights[want]; !ok {
			t.Fatalf("lava fluid heights miss %d: %v", want, lavaHeights)
		}
	}

	soulSand := v2168RecordsNamed(records, "minecraft:soul_sand")
	if len(soulSand) != 1 {
		t.Fatalf("soul_sand states = %d, want 1", len(soulSand))
	}
	if entry := entries[soulSand[0]]; entry.Surface != SurfaceSoulSand || entry.HorizontalSpeed != soulSandSpeedQ1E8 ||
		entry.Friction != 60_000_000 || len(entry.Boxes) != 1 {
		t.Fatalf("soul_sand runtime ID %d facts changed: %+v", soulSand[0], entry)
	}

	web := v2168RecordsNamed(records, "minecraft:web")
	if len(web) != 1 {
		t.Fatalf("web states = %d, want 1", len(web))
	}
	if entry := entries[web[0]]; entry.Flags&(physicsFlagCobweb|physicsFlagPassable) != physicsFlagCobweb|physicsFlagPassable ||
		entry.HorizontalSpeed != 25_000_000 || entry.VerticalSpeed != 5_000_000 || entry.Friction != 60_000_000 || len(entry.Boxes) != 0 {
		t.Fatalf("web runtime ID %d facts changed: %+v", web[0], entry)
	}

	for _, name := range []string{"minecraft:vine"} {
		indexes := v2168RecordsNamed(records, name)
		if len(indexes) != 16 {
			t.Fatalf("%s state count = %d, want 16", name, len(indexes))
		}
		for _, index := range indexes {
			if entry := entries[index]; entry.Flags&physicsFlagClimbable == 0 || len(entry.Boxes) != 0 {
				t.Fatalf("%s runtime ID %d is not climbable empty collision: %+v", name, index, entry)
			}
		}
	}

	ladder := v2168RecordsNamed(records, "minecraft:ladder")
	if len(ladder) != 6 {
		t.Fatalf("ladder states = %d, want 6", len(ladder))
	}
	ladderBoxes := map[int]int{}
	for _, index := range ladder {
		entry := entries[index]
		if entry.Flags&physicsFlagClimbable == 0 {
			t.Fatalf("ladder runtime ID %d lost its climbable fact: %+v", index, entry)
		}
		// The passable bit follows the verbatim seed: empty-seed states are
		// passable, boxed states keep solid collision.
		if len(entry.Boxes) == 0 && entry.Flags&physicsFlagPassable == 0 {
			t.Fatalf("ladder runtime ID %d has an empty seed without passable: %+v", index, entry)
		}
		if len(entry.Boxes) != 0 && entry.Flags&physicsFlagPassable != 0 {
			t.Fatalf("ladder runtime ID %d is passable despite a collision box: %+v", index, entry)
		}
		ladderBoxes[len(entry.Boxes)]++
	}
	// Collision seeds are transplanted verbatim: the reviewed source maps four
	// ladder facings to one box and leaves two empty.
	if ladderBoxes[0] != 2 || ladderBoxes[1] != 4 {
		t.Fatalf("ladder box histogram = %v, want exactly {0:2 1:4}", ladderBoxes)
	}

	powderSnow := v2168RecordsNamed(records, "minecraft:powder_snow")
	if len(powderSnow) != 1 {
		t.Fatalf("powder_snow states = %d, want 1", len(powderSnow))
	}
	if entry := entries[powderSnow[0]]; entry.Flags&(physicsFlagPowderSnow|physicsFlagPassable) != physicsFlagPowderSnow|physicsFlagPassable || len(entry.Boxes) != 0 {
		t.Fatalf("powder_snow runtime ID %d facts changed: %+v", powderSnow[0], entry)
	}

	slime := v2168RecordsNamed(records, "minecraft:slime")
	if len(slime) != 1 {
		t.Fatalf("slime states = %d, want 1", len(slime))
	}
	if entry := entries[slime[0]]; entry.Surface != SurfaceSlime || entry.Friction != 80_000_000 ||
		entry.HorizontalSpeed != defaultSpeedQ1E8 || len(entry.Boxes) != 1 {
		t.Fatalf("slime runtime ID %d facts changed: %+v", slime[0], entry)
	}

	honey := v2168RecordsNamed(records, "minecraft:honey_block")
	if len(honey) != 1 {
		t.Fatalf("honey_block states = %d, want 1", len(honey))
	}
	if entry := entries[honey[0]]; entry.Surface != SurfaceHoney || entry.HorizontalSpeed != unprovenHoneySpeedQ1E8 ||
		entry.Friction != 80_000_000 || len(entry.Boxes) != 1 {
		t.Fatalf("honey_block runtime ID %d facts changed: %+v", honey[0], entry)
	}

	beds := v2168RecordsNamed(records, "minecraft:bed")
	if len(beds) != 16 {
		t.Fatalf("bed states = %d, want 16", len(beds))
	}
	for _, index := range beds {
		if entry := entries[index]; entry.Surface != SurfaceBed {
			t.Fatalf("bed runtime ID %d lost its bounce response: %+v", index, entry)
		}
	}

	bubbleIndexes := v2168RecordsNamed(records, "minecraft:bubble_column")
	if len(bubbleIndexes) != 2 {
		t.Fatalf("bubble_column states = %d, want 2", len(bubbleIndexes))
	}
	bubbleSurfaces := map[SurfaceResponse]int{}
	for _, index := range bubbleIndexes {
		entry := entries[index]
		if entry.Flags&(physicsFlagWater|physicsFlagPassable) != physicsFlagWater|physicsFlagPassable ||
			entry.FluidHeight != defaultSpeedQ1E8 || len(entry.Boxes) != 0 {
			t.Fatalf("bubble_column runtime ID %d lost its fluid facts: %+v", index, entry)
		}
		bubbleSurfaces[entry.Surface]++
	}
	if bubbleSurfaces[SurfaceBubbleUp] != 1 || bubbleSurfaces[SurfaceBubbleDown] != 1 {
		t.Fatalf("bubble directions = %v", bubbleSurfaces)
	}

	stoneIndexes := v2168RecordsNamed(records, "minecraft:stone")
	if len(stoneIndexes) != 1 {
		t.Fatalf("stone states = %d, want 1", len(stoneIndexes))
	}
	stone := entries[stoneIndexes[0]]
	if stone.Friction != 60_000_000 || stone.HorizontalSpeed != defaultSpeedQ1E8 || stone.VerticalSpeed != defaultSpeedQ1E8 ||
		stone.Surface != SurfaceNone || stone.Flags != 0 || len(stone.Boxes) != 1 ||
		stone.Boxes[0] != (CollisionBox{MaxX: 100_000_000, MaxY: 100_000_000, MaxZ: 100_000_000}) {
		t.Fatalf("stone runtime ID %d facts changed: %+v", stoneIndexes[0], stone)
	}

	airIndexes := v2168RecordsNamed(records, "minecraft:air")
	if len(airIndexes) != 1 {
		t.Fatalf("air states = %d, want 1", len(airIndexes))
	}
	air := entries[airIndexes[0]]
	if air.Flags != physicsFlagPassable || len(air.Boxes) != 0 || air.Friction != 90_000_000 {
		t.Fatalf("air runtime ID %d facts changed: %+v", airIndexes[0], air)
	}
}

func TestProjectV2168PhysicsRecordsAppliesLegacyFactsAtNewIdentifiers(t *testing.T) {
	breg, records := loadV2168PhysicsInputs(t)
	pmmp := make(map[string]PMMPLightProperties, len(records))
	for _, record := range records {
		if record.Name == retailReservedName {
			continue
		}
		friction := 0.6
		switch record.Name {
		case "minecraft:slime", "minecraft:honey_block":
			friction = 0.8
		case "minecraft:air":
			friction = 0.9
		}
		pmmp[record.Name] = PMMPLightProperties{Friction: friction}
	}
	sources := syntheticPhysicsSources(records, pmmp)
	physics, err := projectV2168PhysicsRecords(records, sources)
	if err != nil {
		t.Fatal(err)
	}
	if len(physics) != v2168BlockStateCount {
		t.Fatalf("projected physics count = %d, want %d", len(physics), v2168BlockStateCount)
	}

	reserved := 0
	for index, entry := range physics {
		if records[index].Name != retailReservedName {
			continue
		}
		reserved++
		if entry.Flags != physicsFlagPassable || len(entry.Boxes) != 0 || entry.FrictionQ1E8 != defaultSpeedQ1E8 ||
			entry.HorizontalSpeedQ1E8 != defaultSpeedQ1E8 || entry.VerticalSpeedQ1E8 != defaultSpeedQ1E8 ||
			entry.FluidHeightQ1E8 != 0 || entry.SurfaceResponse != SurfaceNone {
			t.Fatalf("reserved runtime ID %d is not neutral: %+v", index, entry)
		}
	}
	if reserved != v2168PhysicsReservedCount {
		t.Fatalf("reserved records = %d, want exactly %d", reserved, v2168PhysicsReservedCount)
	}

	water := physics[v2168RecordsNamed(records, "minecraft:water")[0]]
	if water.Flags&(physicsFlagWater|physicsFlagPassable) != physicsFlagWater|physicsFlagPassable ||
		len(water.Boxes) != 0 || water.FluidHeightQ1E8 <= 0 {
		t.Fatalf("water runtime ID %d lost its fluid facts: %+v", v2168RecordsNamed(records, "minecraft:water")[0], water)
	}
	lava := physics[v2168RecordsNamed(records, "minecraft:lava")[0]]
	if lava.Flags&physicsFlagLava == 0 || lava.Flags&physicsFlagWater != 0 {
		t.Fatalf("lava runtime ID %d lost its lava facts: %+v", v2168RecordsNamed(records, "minecraft:lava")[0], lava)
	}
	soulSand := physics[v2168RecordsNamed(records, "minecraft:soul_sand")[0]]
	if soulSand.SurfaceResponse != SurfaceSoulSand || soulSand.HorizontalSpeedQ1E8 != soulSandSpeedQ1E8 ||
		soulSand.FrictionQ1E8 != 60_000_000 {
		t.Fatalf("soul_sand facts changed: %+v", soulSand)
	}
	honey := physics[v2168RecordsNamed(records, "minecraft:honey_block")[0]]
	if honey.SurfaceResponse != SurfaceHoney || honey.HorizontalSpeedQ1E8 != unprovenHoneySpeedQ1E8 {
		t.Fatalf("honey_block facts changed: %+v", honey)
	}

	again, err := projectV2168PhysicsRecords(records, sources)
	if err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(physics, again) {
		t.Fatal("v2168 physics projection is not deterministic")
	}

	first, err := encodePhysicsRegistryForProtocol(breg, physics, v2168BlockStateCount, v2168BlockProtocol)
	if err != nil {
		t.Fatal(err)
	}
	second, err := encodePhysicsRegistryForProtocol(breg, slices.Clone(physics), v2168BlockStateCount, v2168BlockProtocol)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(first, second) {
		t.Fatal("v2168 physics encoding is not deterministic")
	}
	if got := binary.LittleEndian.Uint32(first[8:12]); got != v2168BlockProtocol {
		t.Fatalf("stamped protocol = %d, want %d", got, v2168BlockProtocol)
	}
	if got := binary.LittleEndian.Uint32(first[12:16]); got != v2168BlockStateCount {
		t.Fatalf("stamped count = %d, want %d", got, v2168BlockStateCount)
	}

	// The shared encoder keeps stamping the legacy protocol unchanged so the
	// protocol-1001 generation path stays byte-reproducible.
	legacy, err := encodePhysicsRegistry([]byte("breg"), []PhysicsRecord{{
		SequentialID: 0, NetworkHash: 1, FrictionQ1E8: 60_000_000,
		HorizontalSpeedQ1E8: defaultSpeedQ1E8, VerticalSpeedQ1E8: defaultSpeedQ1E8, Flags: physicsFlagPassable,
	}}, 1)
	if err != nil {
		t.Fatal(err)
	}
	if got := binary.LittleEndian.Uint32(legacy[8:12]); got != registryProtocol {
		t.Fatalf("legacy encoder protocol = %d, want %d", got, registryProtocol)
	}
}

func TestWriteV2168PhysicsProjectionRejectsWrongProtocolAndMutatedInput(t *testing.T) {
	dir := t.TempDir()
	output := filepath.Join(dir, "block-physics-v2168.bin")
	legacy := filepath.Join("..", "..", "crates", "assets", "data", "block-registry-v1001.bin")
	err := writeV2168PhysicsProjection(legacy, "", "", output, "", "")
	if err == nil || !strings.Contains(err.Error(), "protocol-2168") {
		t.Fatalf("wrong-version rejection = %v", err)
	}

	pinned, err := os.ReadFile(filepath.Join("..", "..", "crates", "assets", "data", "block-registry-v2168.bin"))
	if err != nil {
		t.Fatal(err)
	}
	mutated := append([]byte(nil), pinned...)
	mutated[len(mutated)/2] ^= 0xff
	mutatedPath := filepath.Join(dir, "mutated-breg.bin")
	if err := os.WriteFile(mutatedPath, mutated, 0o600); err != nil {
		t.Fatal(err)
	}
	err = writeV2168PhysicsProjection(mutatedPath, "", "", output, "", "")
	if err == nil || !strings.Contains(err.Error(), "SHA-256") {
		t.Fatalf("mutated-input rejection = %v", err)
	}
	if _, err := os.Stat(output); err == nil {
		t.Fatal("rejected generation still wrote an output")
	}
}

// syntheticV2168PhysicsCorpus builds a full-size v2168 identity space whose
// non-reserved records carry the complete reviewed override families plus
// minecraft:stone padding, so production coverage semantics hold end to end.
func TestV2168PhysicsSeedsTransplantLegacyCollisionVerbatim(t *testing.T) {
	root := filepath.Join("..", "..", "crates", "assets", "data")
	legacyBytes, err := os.ReadFile(filepath.Join(root, "block-registry-v1001.bin"))
	if err != nil {
		t.Fatal(err)
	}
	_, legacy, err := decodeBREGRecords(legacyBytes, registryProtocol)
	if err != nil {
		t.Fatal(err)
	}
	seeds := make(map[string]CollisionSeed, len(legacy))
	for _, record := range legacy {
		if record.Name == retailReservedName {
			continue
		}
		key := canonicalRecordKey(record.Name, record.StateJSON)
		if _, exists := seeds[key]; exists {
			t.Fatalf("duplicate legacy state key %q", key)
		}
		seeds[key] = record.CollisionSeed
	}
	_, records := loadV2168PhysicsInputs(t)
	matched := 0
	for _, record := range records {
		if record.Name == retailReservedName {
			continue
		}
		seed, ok := seeds[canonicalRecordKey(record.Name, record.StateJSON)]
		if !ok {
			t.Fatalf("v2168 state %d (%s) has no legacy counterpart", record.SequentialID, record.Name)
		}
		if seed.ShapeID != record.CollisionSeed.ShapeID || seed.Confidence != record.CollisionSeed.Confidence ||
			!collisionBoxesEqual(seed.Boxes, record.CollisionSeed.Boxes) {
			t.Fatalf("v2168 state %d (%s) mutated the reviewed legacy collision seed", record.SequentialID, record.Name)
		}
		matched++
	}
	if matched != 16_530 {
		t.Fatalf("legacy-exact states = %d, want exactly 16530", matched)
	}
}

func syntheticV2168PhysicsCorpus(reserved int) ([]Record, PhysicsSourceCatalog) {
	records := make([]Record, v2168BlockStateCount)
	for index := 0; index < reserved && index < len(records); index++ {
		records[index] = Record{
			SequentialID: uint32(index),
			NetworkHash:  uint32(index) + 1,
			Name:         retailReservedName,
			StateJSON:    []byte(`{}`),
		}
	}
	pmmp := map[string]PMMPLightProperties{"minecraft:stone": {Friction: 0.6}}
	prismarine := map[string]PrismarinePhysicsFact{}
	dragonfly := map[string][]string{}
	cursor := reserved
	for _, override := range reviewedPhysicsOverrides {
		pmmp[override.Name] = PMMPLightProperties{Friction: 0.6}
		prismarine[strings.TrimPrefix(override.Name, "minecraft:")] = PrismarinePhysicsFact{BoundingBox: override.BoundingBox, StateCount: override.StateCount}
		dragonfly[override.Name] = strings.Split(override.DragonflyTypes, ",")
		for state := 0; state < override.StateCount; state++ {
			if cursor >= len(records) {
				break
			}
			stateJSON := []byte(`{}`)
			if override.Behavior == behaviorBubble {
				// The reviewed bubble route strictly parses one drag_down byte.
				drag := byte(0)
				if state == 1 {
					drag = 1
				}
				stateJSON = []byte(fmt.Sprintf(`{"drag_down":{"type":"byte","value":%d}}`, drag))
			}
			records[cursor] = Record{
				SequentialID: uint32(cursor),
				NetworkHash:  uint32(cursor) + 1,
				Name:         override.Name,
				StateJSON:    stateJSON,
			}
			cursor++
		}
	}
	for ; cursor < len(records); cursor++ {
		records[cursor] = Record{
			SequentialID: uint32(cursor),
			NetworkHash:  uint32(cursor) + 1,
			Name:         "minecraft:stone",
			StateJSON:    []byte(`{}`),
		}
	}
	return records, PhysicsSourceCatalog{PMMP: pmmp, Prismarine: prismarine, DragonflyTypes: dragonfly, RequireProductionCoverage: true}
}

func TestProjectV2168PhysicsFailsClosedListingEveryMissingPMMPName(t *testing.T) {
	records, sources := syntheticV2168PhysicsCorpus(v2168PhysicsReservedCount)
	records[1000].Name = "minecraft:zeta_unlisted"
	records[1001].Name = "minecraft:alpha_unlisted"
	_, err := projectV2168PhysicsRecords(records, sources)
	if err == nil {
		t.Fatal("missing PMMP friction rows were accepted")
	}
	if !strings.Contains(err.Error(), "minecraft:alpha_unlisted") || !strings.Contains(err.Error(), "minecraft:zeta_unlisted") {
		t.Fatalf("failure does not name every missing row: %v", err)
	}
	if strings.Contains(err.Error(), retailReservedName) {
		t.Fatalf("reserved states leaked into the failure listing: %v", err)
	}
	if strings.Index(err.Error(), "minecraft:alpha_unlisted") > strings.Index(err.Error(), "minecraft:zeta_unlisted") {
		t.Fatalf("missing-row listing is not sorted: %v", err)
	}
}

func TestProjectV2168PhysicsRequiresExactlyTheReviewedReservedCount(t *testing.T) {
	undercount, underSources := syntheticV2168PhysicsCorpus(v2168PhysicsReservedCount - 1)
	if _, err := projectV2168PhysicsRecords(undercount, underSources); err == nil ||
		!strings.Contains(err.Error(), "969") {
		t.Fatalf("undercount rejection = %v", err)
	}
	overcount, overSources := syntheticV2168PhysicsCorpus(v2168PhysicsReservedCount + 1)
	if _, err := projectV2168PhysicsRecords(overcount, overSources); err == nil ||
		!strings.Contains(err.Error(), "969") {
		t.Fatalf("overcount rejection = %v", err)
	}

	records, sources := syntheticV2168PhysicsCorpus(v2168PhysicsReservedCount)
	physics, err := projectV2168PhysicsRecords(records, sources)
	if err != nil {
		t.Fatal(err)
	}
	reserved := 0
	stones := 0
	for index, entry := range physics {
		switch records[index].Name {
		case retailReservedName:
			reserved++
			if !v2168PhysicsEntryStructurallyNeutral(entry) {
				t.Fatalf("reserved slot %d is not neutral: %+v", index, entry)
			}
		case "minecraft:stone":
			stones++
			if entry.FrictionQ1E8 != 60_000_000 || entry.Flags != physicsFlagPassable || entry.SurfaceResponse != SurfaceNone {
				t.Fatalf("ordinary slot %d facts changed: %+v", index, entry)
			}
		}
	}
	if reserved != v2168PhysicsReservedCount || stones == 0 {
		t.Fatalf("neutral reserved = %d, stone witnesses = %d", reserved, stones)
	}
}

func v2168PhysicsEntryStructurallyNeutral(entry PhysicsRecord) bool {
	return entry.Flags == physicsFlagPassable && len(entry.Boxes) == 0 && entry.SurfaceResponse == SurfaceNone &&
		entry.FluidHeightQ1E8 == 0 && entry.FrictionQ1E8 == defaultSpeedQ1E8 &&
		entry.HorizontalSpeedQ1E8 == defaultSpeedQ1E8 && entry.VerticalSpeedQ1E8 == defaultSpeedQ1E8
}

func TestV2168PhysicsManifestCrossCheckRejectsDrift(t *testing.T) {
	payload, err := os.ReadFile(filepath.Join("..", "..", "assets", "block-projection-v2168.json"))
	if err != nil {
		t.Fatal(err)
	}
	if err := crossCheckV2168PhysicsManifest(payload); err != nil {
		t.Fatalf("reviewed manifest was rejected: %v", err)
	}

	var generic map[string]any
	if err := json.Unmarshal(payload, &generic); err != nil {
		t.Fatal(err)
	}
	projection := generic["projection"].(map[string]any)
	projection["denied_count"] = 968
	mutated, err := json.Marshal(generic)
	if err != nil {
		t.Fatal(err)
	}
	if err := crossCheckV2168PhysicsManifest(mutated); err == nil || !strings.Contains(err.Error(), "969") {
		t.Fatalf("drifted denial count rejection = %v", err)
	}

	projection["denied_count"] = float64(v2168PhysicsReservedCount)
	generic["output"].(map[string]any)["sha256"] = "0000000000000000000000000000000000000000000000000000000000000000"
	rebound, err := json.Marshal(generic)
	if err != nil {
		t.Fatal(err)
	}
	if err := crossCheckV2168PhysicsManifest(rebound); err == nil || !strings.Contains(err.Error(), "BREG") {
		t.Fatalf("drifted BREG binding rejection = %v", err)
	}
}
