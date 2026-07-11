package main

import (
	"bytes"
	"image/color"
	"math"
	"math/rand"
	"slices"
	"strings"
	"testing"

	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/model"
	"github.com/df-mc/dragonfly/server/world"
)

type registryBiome struct {
	id   int
	name string
}

func (b registryBiome) Temperature() float64    { return 0.8 }
func (b registryBiome) Rainfall() float64       { return 0.4 }
func (b registryBiome) Depth() float64          { return 0.1 }
func (b registryBiome) Scale() float64          { return 0.2 }
func (b registryBiome) WaterColour() color.RGBA { return color.RGBA{} }
func (b registryBiome) Tags() []string          { return nil }
func (b registryBiome) String() string          { return b.name }
func (b registryBiome) EncodeBiome() int        { return b.id }

func TestEncodeBiomeRegistrySortsByStableEncodeBiomeID(t *testing.T) {
	records := []BiomeRecord{
		{ID: 48, Name: "minecraft:bamboo_jungle"},
		{ID: 1, Name: "minecraft:plains"},
	}

	got, err := encodeBiomeRegistry(records)
	if err != nil {
		t.Fatalf("encode biome registry: %v", err)
	}
	want := []byte{
		'B', 'I', 'O', 'R', 'E', 'G', '0', '1',
		0x02, 0x00, 0x00, 0x00,
		0x01, 0x00, 0x00, 0x00,
		0x10, 0x00,
		'm', 'i', 'n', 'e', 'c', 'r', 'a', 'f', 't', ':', 'p', 'l', 'a', 'i', 'n', 's',
		0x30, 0x00, 0x00, 0x00,
		0x17, 0x00,
		'm', 'i', 'n', 'e', 'c', 'r', 'a', 'f', 't', ':', 'b', 'a', 'm', 'b', 'o', 'o', '_', 'j', 'u', 'n', 'g', 'l', 'e',
	}
	if !bytes.Equal(got, want) {
		t.Fatalf("encoded biome bytes:\n got: %x\nwant: %x", got, want)
	}
	if records[0].ID != 48 || records[1].ID != 1 {
		t.Fatal("encodeBiomeRegistry mutated input order")
	}
}

func TestEncodeBiomeRegistryRejectsDuplicatesAndBounds(t *testing.T) {
	tests := []struct {
		name    string
		records []BiomeRecord
		wantErr string
	}{
		{
			name:    "duplicate id",
			records: []BiomeRecord{{ID: 1, Name: "minecraft:plains"}, {ID: 1, Name: "minecraft:other"}},
			wantErr: "duplicate biome ID",
		},
		{
			name:    "duplicate name",
			records: []BiomeRecord{{ID: 1, Name: "minecraft:plains"}, {ID: 2, Name: "minecraft:plains"}},
			wantErr: "duplicate biome name",
		},
		{
			name:    "name bound",
			records: []BiomeRecord{{ID: 1, Name: strings.Repeat("x", maxBiomeNameBytes+1)}},
			wantErr: "biome name too long",
		},
		{
			name:    "empty name",
			records: []BiomeRecord{{ID: 1}},
			wantErr: "biome name is empty",
		},
		{
			name:    "record count",
			records: make([]BiomeRecord, maxBiomeRecordCount+1),
			wantErr: "too many biome records",
		},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			_, err := encodeBiomeRegistry(test.records)
			if err == nil || !strings.Contains(err.Error(), test.wantErr) {
				t.Fatalf("encode error = %v, want %q", err, test.wantErr)
			}
		})
	}
}

func TestCollectBiomesCanonicalizesNamesAndRejectsInvalidIDs(t *testing.T) {
	records, err := collectBiomes([]world.Biome{
		registryBiome{id: 48, name: "bamboo_jungle"},
		registryBiome{id: 900, name: "example:violet_marsh"},
	})
	if err != nil {
		t.Fatalf("collect biomes: %v", err)
	}
	want := []BiomeRecord{
		{ID: 48, Name: "minecraft:bamboo_jungle"},
		{ID: 900, Name: "example:violet_marsh"},
	}
	if !slices.Equal(records, want) {
		t.Fatalf("records = %#v, want %#v", records, want)
	}

	for _, id := range []int{-1, math.MaxUint16 + 1} {
		_, err := collectBiomes([]world.Biome{registryBiome{id: id, name: "invalid"}})
		if err == nil || !strings.Contains(err.Error(), "biome ID") {
			t.Fatalf("collect biome ID %d error = %v", id, err)
		}
	}
}

func TestCollectRegisteredDragonflyBiomesUsesStableNetworkIDs(t *testing.T) {
	records, err := collectBiomes(world.Biomes())
	if err != nil {
		t.Fatalf("collect registered biomes: %v", err)
	}
	if len(records) != 88 {
		t.Fatalf("biome record count = %d, want 88", len(records))
	}
	for _, want := range []BiomeRecord{
		{ID: 0, Name: "minecraft:ocean"},
		{ID: 1, Name: "minecraft:plains"},
		{ID: 48, Name: "minecraft:bamboo_jungle"},
		{ID: 194, Name: "minecraft:sulfur_caves"},
	} {
		found := false
		for _, record := range records {
			if record == want {
				found = true
				break
			}
		}
		if !found {
			t.Errorf("missing Dragonfly biome %#v", want)
		}
	}
}

type classifierBlock struct {
	name       string
	properties map[string]any
	stateHash  uint64
	model      world.BlockModel
}

func (b classifierBlock) EncodeBlock() (string, map[string]any) {
	return b.name, b.properties
}

func (b classifierBlock) Hash() (uint64, uint64) {
	return 1, b.stateHash
}

func (b classifierBlock) Model() world.BlockModel {
	return b.model
}

func TestEncodeSortsRecordsAndMatchesExactBytes(t *testing.T) {
	records := []Record{
		{
			SequentialID: 2,
			NetworkHash:  0xaabbccdd,
			Flags:        flagCubeGeometry | flagOccludesFullFace,
			Name:         "zeta",
			StateJSON:    []byte(`{}`),
		},
		{
			SequentialID: 1,
			NetworkHash:  0x11223344,
			Flags:        flagAir,
			Name:         "alpha",
			StateJSON:    []byte(`{"a":1}`),
		},
	}

	got, err := encode(records)
	if err != nil {
		t.Fatalf("encode: %v", err)
	}
	want := []byte{
		'B', 'R', 'E', 'G', '1', '0', '0', '2',
		0x02, 0x00, 0x00, 0x00,
		0x01, 0x00, 0x00, 0x00,
		0x44, 0x33, 0x22, 0x11,
		0x01,
		0x05, 0x00,
		0x07, 0x00, 0x00, 0x00,
		'a', 'l', 'p', 'h', 'a',
		'{', '"', 'a', '"', ':', '1', '}',
		0x02, 0x00, 0x00, 0x00,
		0xdd, 0xcc, 0xbb, 0xaa,
		0x06,
		0x04, 0x00,
		0x02, 0x00, 0x00, 0x00,
		'z', 'e', 't', 'a',
		'{', '}',
	}
	if !bytes.Equal(got, want) {
		t.Fatalf("encoded bytes:\n got: %x\nwant: %x", got, want)
	}
	if records[0].SequentialID != 2 || records[1].SequentialID != 1 {
		t.Fatal("encode mutated the input order")
	}
}

func TestEncodeRejectsDuplicateSequentialIDs(t *testing.T) {
	records := []Record{
		testRecord(7, 100),
		testRecord(7, 101),
	}
	_, err := encode(records)
	if err == nil || !strings.Contains(err.Error(), "duplicate sequential ID") {
		t.Fatalf("encode error = %v, want duplicate sequential ID", err)
	}
}

func TestEncodeRejectsDuplicateNetworkHashes(t *testing.T) {
	records := []Record{
		testRecord(7, 100),
		testRecord(8, 100),
	}
	_, err := encode(records)
	if err == nil || !strings.Contains(err.Error(), "duplicate network hash") {
		t.Fatalf("encode error = %v, want duplicate network hash", err)
	}
}

func TestEncodeRejectsInvalidFlagSemantics(t *testing.T) {
	tests := []struct {
		name     string
		flags    uint8
		wantFlag string
	}{
		{name: "unknown bit", flags: 1 << 4, wantFlag: "0x10"},
		{name: "air and cube", flags: flagAir | flagCubeGeometry, wantFlag: "0x3"},
		{name: "occluder without cube", flags: flagOccludesFullFace, wantFlag: "0x4"},
		{name: "leaf without cube", flags: flagLeafModel, wantFlag: "0x8"},
		{
			name:     "leaf and occluder",
			flags:    flagCubeGeometry | flagOccludesFullFace | flagLeafModel,
			wantFlag: "0xe",
		},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			record := testRecord(77, 100)
			record.Flags = test.flags
			_, err := encode([]Record{record})
			if err == nil || !strings.Contains(err.Error(), "sequential ID 77") || !strings.Contains(err.Error(), test.wantFlag) {
				t.Fatalf("encode error = %v, want sequential ID 77 and flag %s", err, test.wantFlag)
			}
		})
	}
}

func TestCanonicalJSONSortsPropertyKeys(t *testing.T) {
	got, err := canonicalJSON(map[string]any{
		"z": int32(3),
		"a": map[string]any{"b": int32(2), "a": int32(1)},
		"m": "value",
	})
	if err != nil {
		t.Fatalf("canonical JSON: %v", err)
	}
	want := []byte(`{"a":{"a":1,"b":2},"m":"value","z":3}`)
	if !bytes.Equal(got, want) {
		t.Fatalf("canonical JSON = %s, want %s", got, want)
	}
}

func TestApprovedUnknownFullCubeStateAcceptsOnlyExactPinnedSchemas(t *testing.T) {
	accepted := []struct {
		name       string
		properties map[string]any
	}{
		{name: "minecraft:mycelium", properties: nil},
		{name: "minecraft:mycelium", properties: map[string]any{}},
		{name: "minecraft:red_mushroom_block", properties: map[string]any{"huge_mushroom_bits": int32(0)}},
		{name: "minecraft:red_mushroom_block", properties: map[string]any{"huge_mushroom_bits": int32(15)}},
		{name: "minecraft:brown_mushroom_block", properties: map[string]any{"huge_mushroom_bits": int32(7)}},
		{name: "minecraft:mushroom_stem", properties: map[string]any{"huge_mushroom_bits": int32(12)}},
	}
	for _, test := range accepted {
		if !approvedUnknownFullCubeState(test.name, test.properties) {
			t.Errorf("approvedUnknownFullCubeState(%q, %#v) = false, want true", test.name, test.properties)
		}
	}

	rejected := []struct {
		name       string
		properties map[string]any
	}{
		{name: "minecraft:stone", properties: map[string]any{}},
		{name: "minecraft:red_mushroom", properties: map[string]any{"huge_mushroom_bits": int32(0)}},
		{name: "minecraft:mycelium", properties: map[string]any{"unexpected": int32(0)}},
		{name: "minecraft:red_mushroom_block", properties: nil},
		{name: "minecraft:red_mushroom_block", properties: map[string]any{}},
		{name: "minecraft:red_mushroom_block", properties: map[string]any{"huge_mushroom_bits": int32(0), "unexpected": int32(0)}},
		{name: "minecraft:red_mushroom_block", properties: map[string]any{"huge_mushroom_bits": int32(-1)}},
		{name: "minecraft:red_mushroom_block", properties: map[string]any{"huge_mushroom_bits": int32(16)}},
		{name: "minecraft:red_mushroom_block", properties: map[string]any{"huge_mushroom_bits": uint8(0)}},
		{name: "minecraft:red_mushroom_block", properties: map[string]any{"huge_mushroom_bits": int(0)}},
		{name: "minecraft:red_mushroom_block", properties: map[string]any{"huge_mushroom_bits": float64(0)}},
		{name: "minecraft:red_mushroom_block", properties: map[string]any{"huge_mushroom_bits": "0"}},
		{name: "minecraft:red_mushroom_block", properties: map[string]any{"huge_mushroom_bits": false}},
	}
	for _, test := range rejected {
		if approvedUnknownFullCubeState(test.name, test.properties) {
			t.Errorf("approvedUnknownFullCubeState(%q, %#v) = true, want false", test.name, test.properties)
		}
	}
}

func TestClassifyFlagsSeparatesSolidLeavesAndOtherModels(t *testing.T) {
	if got := classifyFlags(block.Stone{}); got != flagCubeGeometry|flagOccludesFullFace {
		t.Fatalf("stone flags = %#x", got)
	}
	leaf := block.Leaves{Type: block.CherryLeaves(), Persistent: true}
	if got := classifyFlags(leaf); got != flagCubeGeometry|flagLeafModel {
		t.Fatalf("cherry leaf flags = %#x", got)
	}
	if got := classifyFlags(block.Torch{}); got != 0 {
		t.Fatalf("torch flags = %#x", got)
	}
}

func TestClassifyFlagsPreservesSolidAndFailsClosedOverrides(t *testing.T) {
	tests := []struct {
		name  string
		block classifierBlock
		want  uint8
	}{
		{
			name:  "implemented solid",
			block: classifierBlock{name: "minecraft:stone", stateHash: 1, model: model.Solid{}},
			want:  flagCubeGeometry | flagOccludesFullFace,
		},
		{
			name:  "approved unknown",
			block: classifierBlock{name: "minecraft:mycelium", stateHash: math.MaxUint64, model: model.Empty{}},
			want:  flagCubeGeometry | flagOccludesFullFace,
		},
		{
			name:  "implemented non-solid target",
			block: classifierBlock{name: "minecraft:mycelium", stateHash: 1, model: model.Empty{}},
			want:  0,
		},
		{
			name:  "unapproved unknown",
			block: classifierBlock{name: "minecraft:acacia_sapling", stateHash: math.MaxUint64, model: model.Empty{}},
			want:  0,
		},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if got := classifyFlags(test.block); got != test.want {
				t.Fatalf("classifyFlags() = %#x, want %#x", got, test.want)
			}
		})
	}
}

func TestEncodeIsStableAcrossShuffledInputs(t *testing.T) {
	validFlags := []uint8{
		0,
		flagAir,
		flagCubeGeometry,
		flagCubeGeometry | flagOccludesFullFace,
		flagCubeGeometry | flagLeafModel,
	}
	records := make([]Record, 16)
	for i := range records {
		records[i] = Record{
			SequentialID: uint32(i),
			NetworkHash:  uint32(10_000 + i),
			Flags:        validFlags[i%len(validFlags)],
			Name:         "minecraft:block_" + string(rune('a'+i)),
			StateJSON:    []byte(`{"value":1}`),
		}
	}
	want, err := encode(records)
	if err != nil {
		t.Fatalf("encode baseline: %v", err)
	}

	rng := rand.New(rand.NewSource(42))
	for iteration := 0; iteration < 100; iteration++ {
		shuffled := append([]Record(nil), records...)
		rng.Shuffle(len(shuffled), func(i, j int) {
			shuffled[i], shuffled[j] = shuffled[j], shuffled[i]
		})
		got, err := encode(shuffled)
		if err != nil {
			t.Fatalf("encode shuffle %d: %v", iteration, err)
		}
		if !bytes.Equal(got, want) {
			t.Fatalf("shuffle %d produced different bytes", iteration)
		}
	}
}

func TestEncodeRejectsOversizedInputs(t *testing.T) {
	tests := []struct {
		name    string
		records []Record
		wantErr string
	}{
		{
			name:    "record count",
			records: make([]Record, maxRecordCount+1),
			wantErr: "too many records",
		},
		{
			name: "name",
			records: []Record{{
				SequentialID: 1,
				NetworkHash:  2,
				Name:         strings.Repeat("a", maxNameBytes+1),
				StateJSON:    []byte(`{}`),
			}},
			wantErr: "name too long",
		},
		{
			name: "state",
			records: []Record{{
				SequentialID: 1,
				NetworkHash:  2,
				Name:         "minecraft:test",
				StateJSON:    bytes.Repeat([]byte{'x'}, maxStateBytes+1),
			}},
			wantErr: "state payload too large",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			_, err := encode(tt.records)
			if err == nil || !strings.Contains(err.Error(), tt.wantErr) {
				t.Fatalf("encode error = %v, want %q", err, tt.wantErr)
			}
		})
	}
}

func TestCollectDefaultBlockRegistry(t *testing.T) {
	records, err := collect(world.DefaultBlockRegistry)
	if err != nil {
		t.Fatalf("collect registry: %v", err)
	}
	if len(records) != 16_913 {
		t.Fatalf("registry record count = %d, want 16913", len(records))
	}
	air := findByName(t, records, "minecraft:air")
	if air.Flags != flagAir {
		t.Fatalf("air flags = %#x, want %#x", air.Flags, flagAir)
	}
	if air.NetworkHash != 0xdbf44120 {
		t.Fatalf("air hash = %#x", air.NetworkHash)
	}
	stone := findByName(t, records, "minecraft:stone")
	if stone.Flags != flagCubeGeometry|flagOccludesFullFace {
		t.Fatalf("stone flags = %#x, want cube+occluder", stone.Flags)
	}

	var cubeCount, occluderCount, leafCount, airCount int
	leafNames := map[string]struct{}{}
	for _, record := range records {
		if !validRecordFlags(record.Flags) {
			t.Fatalf("record %d has invalid flags %#x", record.SequentialID, record.Flags)
		}
		if record.Flags&flagCubeGeometry != 0 {
			cubeCount++
		}
		if record.Flags&flagOccludesFullFace != 0 {
			occluderCount++
		}
		if record.Flags&flagLeafModel != 0 {
			leafCount++
			leafNames[record.Name] = struct{}{}
			want := flagCubeGeometry | flagLeafModel
			if record.Flags != want {
				t.Errorf("leaf %s state %s flags = %#x, want %#x", record.Name, record.StateJSON, record.Flags, want)
			}
		}
		if record.Flags&flagAir != 0 {
			airCount++
		}
	}
	if cubeCount != 713 || occluderCount != 669 || leafCount != 44 || airCount != 1 {
		t.Fatalf("flag counts cube=%d occluder=%d leaf=%d air=%d, want 713/669/44/1", cubeCount, occluderCount, leafCount, airCount)
	}
	wantLeafNames := []string{
		"minecraft:acacia_leaves",
		"minecraft:azalea_leaves",
		"minecraft:azalea_leaves_flowered",
		"minecraft:birch_leaves",
		"minecraft:cherry_leaves",
		"minecraft:dark_oak_leaves",
		"minecraft:jungle_leaves",
		"minecraft:mangrove_leaves",
		"minecraft:oak_leaves",
		"minecraft:pale_oak_leaves",
		"minecraft:spruce_leaves",
	}
	if len(leafNames) != len(wantLeafNames) {
		t.Fatalf("distinct leaf names = %d, want %d: %#v", len(leafNames), len(wantLeafNames), leafNames)
	}
	for _, name := range wantLeafNames {
		if _, ok := leafNames[name]; !ok {
			t.Errorf("leaf name %s is absent", name)
		}
	}

	for name, wantCount := range map[string]int{
		"minecraft:mycelium":             1,
		"minecraft:red_mushroom_block":   16,
		"minecraft:brown_mushroom_block": 16,
		"minecraft:mushroom_stem":        16,
	} {
		matches := findAllByName(records, name)
		if len(matches) != wantCount {
			t.Errorf("%s state count = %d, want %d", name, len(matches), wantCount)
			continue
		}
		for _, record := range matches {
			if record.Flags != flagCubeGeometry|flagOccludesFullFace {
				t.Errorf("%s state %s flags = %#x, want cube+occluder", name, record.StateJSON, record.Flags)
			}
		}
	}

	for _, name := range []string{
		"minecraft:acacia_sapling",
		"minecraft:cactus_flower",
		"minecraft:flower_pot",
		"minecraft:iron_door",
		"minecraft:iron_trapdoor",
	} {
		matches := findAllByName(records, name)
		if len(matches) == 0 {
			t.Errorf("negative-control block %s is absent", name)
			continue
		}
		for _, record := range matches {
			if record.Flags&flagCubeGeometry != 0 {
				t.Errorf("negative-control block %s state %s was marked cube geometry", name, record.StateJSON)
			}
		}
	}
}

func testRecord(sequentialID, networkHash uint32) Record {
	return Record{
		SequentialID: sequentialID,
		NetworkHash:  networkHash,
		Name:         "minecraft:test",
		StateJSON:    []byte(`{}`),
	}
}

func findByName(t *testing.T, records []Record, name string) Record {
	t.Helper()
	for _, record := range records {
		if record.Name == name {
			return record
		}
	}
	t.Fatalf("record %q not found", name)
	return Record{}
}

func findAllByName(records []Record, name string) []Record {
	var matches []Record
	for _, record := range records {
		if record.Name == name {
			matches = append(matches, record)
		}
	}
	return matches
}
