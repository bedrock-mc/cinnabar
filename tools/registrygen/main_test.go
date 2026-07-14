package main

import (
	"bytes"
	"encoding/binary"
	"encoding/hex"
	"image/color"
	"math"
	"math/rand"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/model"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/sandertv/gophertunnel/minecraft/nbt"
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
	want, err := hex.DecodeString("4252454731303033e90300000200000002000000000000000000000002000000020000000100000044332211010000000000020000000500070000000000000000000000000000000000000000000000000000000000000000000000616c7068617b2261223a317d02000000ddccbbaa0600000000000200000004000200000000000000000000000000000000000000000000000000000000000000000000007a6574617b7d")
	if err != nil {
		t.Fatalf("decode expected BREG1003 fixture: %v", err)
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

func byteState(name string, value byte) StateProperty {
	return StateProperty{Name: name, Value: TypedScalar{Kind: ScalarByte, Byte: value}}
}

func intState(name string, value int32) StateProperty {
	return StateProperty{Name: name, Value: TypedScalar{Kind: ScalarInt, Int: value}}
}

func sourceState(name string, properties ...StateProperty) SourceState {
	return SourceState{Name: name, Properties: properties}
}

func TestJoinSourcesBijection(t *testing.T) {
	pmmp := []SourceState{
		sourceState("minecraft:test", intState("level", 2), byteState("open", 1)),
		sourceState("minecraft:air"),
	}
	dragonfly := []SourceState{
		sourceState("minecraft:air"),
		// Property order is deliberately reversed. Identity is the sorted typed
		// compound, never input order.
		sourceState("minecraft:test", byteState("open", 1), intState("level", 2)),
	}
	prismarine := []SourceState{
		sourceState("minecraft:test", byteState("open", 1), intState("level", 2)),
		sourceState("minecraft:air"),
	}

	joined, err := joinSources(pmmp, dragonfly, prismarine, canonicalStateHash)
	if err != nil {
		t.Fatalf("join sources: %v", err)
	}
	if len(joined) != 2 {
		t.Fatalf("joined records = %d, want 2", len(joined))
	}
	if got, want := string(joined[0].StateJSON), `{}`; got != want {
		t.Fatalf("air state = %s, want %s", got, want)
	}
	if got, want := string(joined[1].StateJSON), `{"level":{"type":"int","value":2},"open":{"type":"byte","value":1}}`; got != want {
		t.Fatalf("typed state = %s, want %s", got, want)
	}

	t.Run("duplicate property", func(t *testing.T) {
		bad := []SourceState{sourceState("minecraft:test", intState("level", 1), intState("level", 1))}
		_, err := joinSources(bad, []SourceState{sourceState("minecraft:test", intState("level", 1))}, []SourceState{sourceState("minecraft:test", intState("level", 1))}, canonicalStateHash)
		if err == nil || !strings.Contains(err.Error(), "duplicate state property") {
			t.Fatalf("error = %v, want duplicate state property", err)
		}
	})

	t.Run("missing and extra records", func(t *testing.T) {
		_, err := joinSources(pmmp, dragonfly[:1], prismarine, canonicalStateHash)
		if err == nil || !strings.Contains(err.Error(), "missing from Dragonfly") {
			t.Fatalf("missing error = %v", err)
		}
		extra := append(append([]SourceState(nil), prismarine...), sourceState("minecraft:extra"))
		_, err = joinSources(pmmp, dragonfly, extra, canonicalStateHash)
		if err == nil || !strings.Contains(err.Error(), "extra in Prismarine") {
			t.Fatalf("extra error = %v", err)
		}
	})

	t.Run("deliberate canonical hash collision", func(t *testing.T) {
		constantHash := func([]byte) uint64 { return 7 }
		_, err := joinSources(pmmp, dragonfly, prismarine, constantHash)
		if err == nil || !strings.Contains(err.Error(), "canonical state hash collision") {
			t.Fatalf("collision error = %v", err)
		}
	})
}

func TestJoinSourcesRejectsTypedStateMismatch(t *testing.T) {
	pmmp := []SourceState{sourceState("minecraft:test", intState("value", 1))}

	tests := []struct {
		name       string
		dragonfly  []SourceState
		prismarine []SourceState
	}{
		{
			name:       "scalar type",
			dragonfly:  []SourceState{sourceState("minecraft:test", byteState("value", 1))},
			prismarine: pmmp,
		},
		{
			name:       "unequal value",
			dragonfly:  []SourceState{sourceState("minecraft:test", intState("value", 2))},
			prismarine: pmmp,
		},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			_, err := joinSources(pmmp, test.dragonfly, test.prismarine, canonicalStateHash)
			if err == nil || !strings.Contains(err.Error(), "typed state mismatch") {
				t.Fatalf("error = %v, want typed state mismatch", err)
			}
		})
	}
}

func TestValentineSubsetAudit(t *testing.T) {
	canonical := []SourceState{
		sourceState("minecraft:air"),
		sourceState("minecraft:new_block", intState("age", 0)),
		sourceState("minecraft:test", byteState("open", 0)),
		sourceState("minecraft:test", byteState("open", 1)),
	}
	valentine := []SourceState{
		sourceState("minecraft:air"),
		sourceState("minecraft:test", byteState("open", 0)),
		sourceState("minecraft:test", byteState("open", 1)),
	}

	audit, err := auditValentineSubset(canonical, valentine)
	if err != nil {
		t.Fatalf("audit Valentine subset: %v", err)
	}
	if audit.CanonicalStates != 4 || audit.ValentineStates != 3 || audit.GapStates != 1 {
		t.Fatalf("state counts = %+v", audit)
	}
	if audit.CanonicalNames != 3 || audit.ValentineNames != 2 || audit.GapNames != 1 {
		t.Fatalf("name counts = %+v", audit)
	}
	if !slices.Equal(audit.MissingNames, []string{"minecraft:new_block"}) {
		t.Fatalf("missing names = %#v", audit.MissingNames)
	}
	if audit.Joined != 3 || audit.Missing != 1 || audit.Extra != 0 || audit.Mismatched != 0 {
		t.Fatalf("join disposition = %+v", audit)
	}

	reordered := []SourceState{valentine[2], valentine[0], valentine[1]}
	_, err = auditValentineSubset(canonical, reordered)
	if err == nil || !strings.Contains(err.Error(), "Valentine overlap order") {
		t.Fatalf("order error = %v", err)
	}
	if len(audit.MissingStates) != 1 || !strings.Contains(audit.MissingStates[0], "minecraft:new_block") {
		t.Fatalf("missing states = %#v", audit.MissingStates)
	}

	bad := append([]SourceState(nil), valentine...)
	bad[0] = sourceState("minecraft:test", intState("open", 1))
	_, err = auditValentineSubset(canonical, bad)
	if err == nil || !strings.Contains(err.Error(), "Valentine typed state") {
		t.Fatalf("mismatch error = %v", err)
	}
}

func TestFileSHA256ReturnsReadError(t *testing.T) {
	if _, err := fileSHA256(filepath.Join(t.TempDir(), "missing.bin")); err == nil {
		t.Fatal("fileSHA256 accepted a missing source")
	}
}

func TestSelectorCardinality(t *testing.T) {
	var stairs []Record
	for upsideDown := byte(0); upsideDown < 2; upsideDown++ {
		for facing := int32(0); facing < 4; facing++ {
			r, err := classifyRecord(sourceState(
				"minecraft:oak_stairs",
				intState("weirdo_direction", facing),
				byteState("upside_down_bit", upsideDown),
			))
			if err != nil {
				t.Fatalf("classify stair: %v", err)
			}
			stairs = append(stairs, r)
		}
	}
	if err := validateSelectorCardinality(stairs); err != nil {
		t.Fatalf("stair selector: %v", err)
	}
	for _, record := range stairs {
		if record.ModelFamily != ModelFamilyStair || record.ContributorRole != ContributorPrimary {
			t.Fatalf("stair classification = family %v role %v", record.ModelFamily, record.ContributorRole)
		}
		if record.FaceCoverage != 0 {
			t.Fatalf("stair coverage = %#x, want conservative empty", record.FaceCoverage)
		}
	}

	fixtures := []struct {
		state  SourceState
		family ModelFamily
		role   ContributorRole
		field  ModelStateField
		value  uint32
	}{
		{sourceState("minecraft:short_grass"), ModelFamilyCross, ContributorPrimary, 0, 0},
		{sourceState("minecraft:fern"), ModelFamilyCross, ContributorPrimary, 0, 0},
		{sourceState("minecraft:red_flower", intState("flower_type", 0)), ModelFamilyCross, ContributorPrimary, 0, 0},
		{sourceState("minecraft:wheat", intState("growth", 7)), ModelFamilyCrop, ContributorPrimary, ModelStateGrowth, 7},
		{sourceState("minecraft:melon_stem", intState("growth", 4)), ModelFamilyCrop, ContributorPrimary, ModelStateGrowth, 4},
		{sourceState("minecraft:water", intState("liquid_depth", 15)), ModelFamilyLiquid, ContributorLiquidAdditional, ModelStateLiquidDepth, 15},
		{sourceState("minecraft:flowing_water", intState("liquid_depth", 3)), ModelFamilyLiquid, ContributorLiquidAdditional, ModelStateLiquidDepth, 3},
		{sourceState("minecraft:stone_block_slab", intState("vertical_half", 0)), ModelFamilySlab, ContributorPrimary, ModelStateHalf, 0},
		{sourceState("minecraft:stone_block_slab", intState("vertical_half", 1)), ModelFamilySlab, ContributorPrimary, ModelStateHalf, 1},
		{sourceState("minecraft:double_stone_block_slab"), ModelFamilySlab, ContributorPrimary, ModelStateHalf, 2},
		{sourceState("minecraft:cinnabar_slab", StateProperty{Name: "minecraft:vertical_half", Value: TypedScalar{Kind: ScalarString, String: "top"}}), ModelFamilySlab, ContributorPrimary, ModelStateHalf, 1},
		{sourceState("minecraft:test", StateProperty{Name: "cardinal_direction", Value: TypedScalar{Kind: ScalarString, String: "north"}}), ModelFamilyUnknown, ContributorPrimary, ModelStateOrientation, 2},
		{sourceState("minecraft:vine", intState("vine_direction_bits", 9)), ModelFamilyVine, ContributorPrimary, ModelStateConnections, 9},
		{sourceState("minecraft:iron_bars"), ModelFamilyPane, ContributorPrimary, 0, 0},
		{sourceState("minecraft:red_bed", intState("direction", 2)), ModelFamilyBed, ContributorPrimary, ModelStateOrientation, 2},
		{sourceState("minecraft:dandelion"), ModelFamilyCross, ContributorPrimary, 0, 0},
		{sourceState("minecraft:tall_seagrass"), ModelFamilyAquatic, ContributorPrimary, 0, 0},
		{sourceState("minecraft:cocoa", intState("age", 2)), ModelFamilyCocoa, ContributorPrimary, ModelStateGrowth, 2},
		{sourceState("minecraft:tube_coral_block"), ModelFamilyUnknown, ContributorPrimary, 0, 0},
		{sourceState("minecraft:sea_pickle", intState("cluster_count", 2)), ModelFamilyUnknown, ContributorPrimary, 0, 0},
		{sourceState("minecraft:copper_bars"), ModelFamilyPane, ContributorPrimary, 0, 0},
		{sourceState("minecraft:torchflower"), ModelFamilyCross, ContributorPrimary, 0, 0},
		{sourceState("minecraft:lever", StateProperty{Name: "lever_direction", Value: TypedScalar{Kind: ScalarString, String: "north"}}), ModelFamilyLever, ContributorPrimary, ModelStateOrientation, 4},
		{sourceState("minecraft:barrier"), ModelFamilyInvisible, ContributorPrimary, 0, 0},
		{sourceState("minecraft:dragon_egg"), ModelFamilyDecorative, ContributorPrimary, 0, 0},
		{sourceState("minecraft:brown_mushroom"), ModelFamilyCross, ContributorPrimary, 0, 0},
		{sourceState("minecraft:deadbush"), ModelFamilyCross, ContributorPrimary, 0, 0},
		{sourceState("minecraft:cave_vines", intState("growing_plant_age", 4)), ModelFamilyCross, ContributorPrimary, 0, 0},
		{sourceState("minecraft:white_glazed_terracotta", intState("facing_direction", 2)), ModelFamilyCube, ContributorPrimary, ModelStateOrientation, 2},
		{sourceState("minecraft:chorus_flower", intState("age", 3)), ModelFamilyUnknown, ContributorPrimary, ModelStateGrowth, 3},
		{sourceState("minecraft:waxed_copper_golem_statue", intState("copper_golem_pose", 1)), ModelFamilyStatue, ContributorPrimary, 0, 0},
		{sourceState("minecraft:soul_sand"), ModelFamilyCuboid, ContributorPrimary, 0, 0},
		{sourceState("minecraft:mud"), ModelFamilyCuboid, ContributorPrimary, 0, 0},
		{sourceState("minecraft:cobblestone_wall",
			StateProperty{Name: "wall_connection_type_north", Value: TypedScalar{Kind: ScalarString, String: "short"}},
			StateProperty{Name: "wall_connection_type_east", Value: TypedScalar{Kind: ScalarString, String: "tall"}},
			StateProperty{Name: "wall_connection_type_south", Value: TypedScalar{Kind: ScalarString, String: "none"}},
			StateProperty{Name: "wall_connection_type_west", Value: TypedScalar{Kind: ScalarString, String: "short"}},
			byteState("wall_post_bit", 1)), ModelFamilyWall, ContributorPrimary, ModelStateConnections, 329},
	}
	for _, fixture := range fixtures {
		record, err := classifyRecord(fixture.state)
		if err != nil {
			t.Fatalf("classify %s: %v", fixture.state.Name, err)
		}
		if record.ModelFamily != fixture.family || record.ContributorRole != fixture.role {
			t.Errorf("%s = family %v role %v", fixture.state.Name, record.ModelFamily, record.ContributorRole)
		}
		if fixture.field != 0 {
			value, ok := record.ModelState.Get(fixture.field)
			if !ok || value != fixture.value {
				t.Errorf("%s field %v = %d/%v, want %d", fixture.state.Name, fixture.field, value, ok, fixture.value)
			}
		}
	}

	broken := append([]Record(nil), stairs[:7]...)
	if err := validateSelectorCardinality(broken); err == nil || !strings.Contains(err.Error(), "selector cardinality") {
		t.Fatalf("broken selector error = %v", err)
	}
}

func TestVineClassificationCoversEveryProtocol1001DirectionMask(t *testing.T) {
	for mask := uint32(0); mask < 16; mask++ {
		record, err := classifyRecord(sourceState("minecraft:vine", intState("vine_direction_bits", int32(mask))))
		if err != nil {
			t.Fatalf("mask %d: classify: %v", mask, err)
		}
		if record.ModelFamily != ModelFamilyVine {
			t.Errorf("mask %d: family=%v, want Vine", mask, record.ModelFamily)
		}
		connections, ok := record.ModelState.Get(ModelStateConnections)
		if !ok || connections != mask {
			t.Errorf("mask %d: connections=(%d, %t)", mask, connections, ok)
		}
		if record.Flags&(flagCubeGeometry|flagOccludesFullFace) != 0 || record.FaceCoverage != 0 {
			t.Errorf("mask %d: vine acquired full-block geometry/occlusion: flags=%#x coverage=%#x", mask, record.Flags, record.FaceCoverage)
		}
	}
}

func TestStairSelectorUsesCanonicalLogicalFacingAndUpsideDown(t *testing.T) {
	wantFacing := map[int32]uint32{0: 0, 1: 1, 2: 2, 3: 3} // south, west, north, east
	for raw, want := range wantFacing {
		for upside := byte(0); upside < 2; upside++ {
			record, err := classifyRecord(sourceState(
				"minecraft:oak_stairs",
				intState("weirdo_direction", raw),
				byteState("upside_down_bit", upside),
			))
			if err != nil {
				t.Fatalf("classify direction=%d upside=%d: %v", raw, upside, err)
			}
			if got, ok := record.ModelState.Get(ModelStateOrientation); !ok || got != want {
				t.Fatalf("direction=%d orientation=%d/%v, want %d/true", raw, got, ok, want)
			}
			if got, ok := record.ModelState.Get(ModelStateHalf); !ok || got != uint32(upside) {
				t.Fatalf("direction=%d upside=%d half=%d/%v", raw, upside, got, ok)
			}
			if record.FaceCoverage != 0 || record.Flags&(flagCubeGeometry|flagOccludesFullFace) != 0 {
				t.Fatalf("direction=%d upside=%d stair was promoted to full coverage: %#x/%#x", raw, upside, record.FaceCoverage, record.Flags)
			}
		}
	}
}

func TestFlowerBedClassificationPreservesGrowthAndOrientation(t *testing.T) {
	for _, name := range []string{"minecraft:wildflowers", "minecraft:pink_petals"} {
		state := sourceState(name,
			intState("growth", 2),
			StateProperty{Name: "minecraft:cardinal_direction", Value: TypedScalar{Kind: ScalarString, String: "east"}},
		)
		record, err := classifyRecord(state)
		if err != nil {
			t.Fatalf("classify %s: %v", name, err)
		}
		if record.ModelFamily != ModelFamilyFlowerBed {
			t.Fatalf("%s family=%v, want FlowerBed", name, record.ModelFamily)
		}
		if got, ok := record.ModelState.Get(ModelStateGrowth); !ok || got != 2 {
			t.Fatalf("%s growth=%d/%v, want 2/true", name, got, ok)
		}
		if got, ok := record.ModelState.Get(ModelStateOrientation); !ok || got != 3 {
			t.Fatalf("%s orientation=%d/%v, want east (3/true)", name, got, ok)
		}
	}
}

func TestFlowerBedCanonicalStateCoverage(t *testing.T) {
	directions := []string{"south", "west", "north", "east"}
	for _, name := range []string{"minecraft:wildflowers", "minecraft:pink_petals"} {
		records := make([]Record, 0, 32)
		states := make(map[string]struct{}, 32)
		selectors := make(map[[2]uint32]struct{}, 32)
		for growth := int32(0); growth < 8; growth++ {
			for _, direction := range directions {
				record, err := classifyRecord(sourceState(name,
					intState("growth", growth),
					StateProperty{Name: "minecraft:cardinal_direction", Value: TypedScalar{Kind: ScalarString, String: direction}},
				))
				if err != nil {
					t.Fatalf("classify %s growth=%d direction=%s: %v", name, growth, direction, err)
				}
				if record.ModelFamily == ModelFamilyCross || record.ModelFamily == ModelFamilyUnknown {
					t.Fatalf("%s growth=%d direction=%s family=%v, want dedicated family", name, growth, direction, record.ModelFamily)
				}
				if record.ModelFamily != ModelFamilyFlowerBed {
					t.Fatalf("%s growth=%d direction=%s family=%v, want FlowerBed", name, growth, direction, record.ModelFamily)
				}
				gotGrowth, hasGrowth := record.ModelState.Get(ModelStateGrowth)
				orientation, hasOrientation := record.ModelState.Get(ModelStateOrientation)
				if !hasGrowth || !hasOrientation {
					t.Fatalf("%s growth=%d direction=%s selector mask=%#x", name, growth, direction, record.ModelState.Mask)
				}
				records = append(records, record)
				states[string(record.StateJSON)] = struct{}{}
				selectors[[2]uint32{gotGrowth, orientation}] = struct{}{}
			}
		}
		if len(records) != 32 || len(states) != 32 || len(selectors) != 32 {
			t.Fatalf("%s records/states/selectors=%d/%d/%d, want 32/32/32", name, len(records), len(states), len(selectors))
		}
	}
}

func TestEncodeBREG1003Canonical(t *testing.T) {
	record, err := classifyRecord(sourceState("minecraft:stone"))
	if err != nil {
		t.Fatalf("classify record: %v", err)
	}
	record.SequentialID = 0
	record.NetworkHash = 0x11223344
	record.Flags = flagCubeGeometry | flagOccludesFullFace
	record.CollisionSeed = CollisionSeed{
		ShapeID:    7,
		Confidence: CollisionConfidenceCollisionOnly,
		Boxes:      []CollisionBox{{MinX: 0, MinY: 0, MinZ: 0, MaxX: 4096, MaxY: 4096, MaxZ: 4096}},
	}
	record.Provenance = ProvenancePMMP | ProvenanceDragonfly | ProvenancePrismarine | ProvenanceValentine

	metadata := RegistryMetadata{
		Protocol:           1001,
		CanonicalNames:     1,
		CanonicalStates:    1,
		ValentineNames:     1,
		ValentineStates:    1,
		ValentineGapNames:  0,
		ValentineGapStates: 0,
	}
	first, err := encodeWithMetadata(metadata, []Record{record})
	if err != nil {
		t.Fatalf("encode: %v", err)
	}
	second, err := encodeWithMetadata(metadata, []Record{record})
	if err != nil {
		t.Fatalf("encode repeat: %v", err)
	}
	if !bytes.Equal(first, second) {
		t.Fatal("BREG1003 encoding is not deterministic")
	}
	if got := string(first[:8]); got != "BREG1003" {
		t.Fatalf("magic = %q", got)
	}
	if got := binary.LittleEndian.Uint32(first[8:12]); got != 1001 {
		t.Fatalf("protocol = %d", got)
	}
}

func TestEncodeBREG1003AcceptsAquaticFamily(t *testing.T) {
	record := testRecord(0, 1)
	record.ModelFamily = ModelFamilyAquatic
	record.Provenance = ProvenancePMMP | ProvenanceDragonfly | ProvenancePrismarine
	if _, err := encode([]Record{record}); err != nil {
		t.Fatalf("encode aquatic family: %v", err)
	}
}

func TestEncodeBREG1003RejectsValentineProvenanceMetadataMismatch(t *testing.T) {
	records := []Record{testRecord(0, 1), testRecord(1, 2)}
	records[0].Name = "minecraft:first"
	records[1].Name = "minecraft:second"
	for i := range records {
		records[i].Provenance = ProvenancePMMP | ProvenanceDragonfly | ProvenancePrismarine | ProvenanceValentine
	}
	metadata := RegistryMetadata{Protocol: 1001, CanonicalNames: 2, CanonicalStates: 2, ValentineNames: 1, ValentineStates: 2, ValentineGapNames: 1}
	if _, err := encodeWithMetadata(metadata, records); err == nil || !strings.Contains(err.Error(), "Valentine provenance name count") {
		t.Fatalf("name mismatch error = %v", err)
	}
	metadata.ValentineNames = 2
	metadata.ValentineGapNames = 0
	metadata.ValentineStates = 1
	metadata.ValentineGapStates = 1
	if _, err := encodeWithMetadata(metadata, records); err == nil || !strings.Contains(err.Error(), "Valentine provenance state count") {
		t.Fatalf("state mismatch error = %v", err)
	}
}

func TestDecodeNBTStatesStopsAtExactBufferEnd(t *testing.T) {
	first, err := nbt.Marshal(nbtState{Name: "minecraft:air", Properties: map[string]any{}, Version: 1})
	if err != nil {
		t.Fatalf("marshal first NBT state: %v", err)
	}
	second, err := nbt.Marshal(nbtState{Name: "minecraft:test", Properties: map[string]any{"open": uint8(1)}, Version: 1})
	if err != nil {
		t.Fatalf("marshal second NBT state: %v", err)
	}
	states, err := decodeNBTStates(append(first, second...))
	if err != nil {
		t.Fatalf("decode exact NBT stream: %v", err)
	}
	if len(states) != 2 || states[1].Name != "minecraft:test" {
		t.Fatalf("decoded states = %#v", states)
	}
}

func TestCollisionSeedPreservesPinnedDecimalCoordinates(t *testing.T) {
	seed, err := collisionSeed(35, map[string][][]float64{
		"35": {{0.025, -0.0625, 0.1, 0.95000005, 1.0, 0.9}},
	})
	if err != nil {
		t.Fatalf("collision seed: %v", err)
	}
	want := CollisionBox{MinX: 2_500_000, MinY: -6_250_000, MinZ: 10_000_000, MaxX: 95_000_005, MaxY: 100_000_000, MaxZ: 90_000_000}
	if len(seed.Boxes) != 1 || seed.Boxes[0] != want {
		t.Fatalf("collision boxes = %#v, want %#v", seed.Boxes, want)
	}
}

func TestCollisionShapeVariantOrdering(t *testing.T) {
	ids := []uint16{8, 3, 5}
	for occurrence, want := range ids {
		got, err := collisionShapeID(ids, occurrence, len(ids))
		if err != nil || got != want {
			t.Fatalf("variant %d = %d, %v; want %d", occurrence, got, err, want)
		}
	}
	if got, err := collisionShapeID([]uint16{7}, 2, 4); err != nil || got != 7 {
		t.Fatalf("shared shape = %d, %v", got, err)
	}
	if _, err := collisionShapeID([]uint16{1, 2}, 0, 3); err == nil || !strings.Contains(err.Error(), "variant cardinality") {
		t.Fatalf("cardinality error = %v", err)
	}
}

func TestValidateRealProvenance(t *testing.T) {
	records := make([]Record, 4)
	for i := range records {
		records[i].Provenance = ProvenancePMMP | ProvenanceDragonfly | ProvenancePrismarine
		if i < 3 {
			records[i].Provenance |= ProvenanceValentine
		}
	}
	audit := ValentineAudit{CanonicalStates: 4, ValentineStates: 3, Joined: 3, Missing: 1}
	if err := validateRealProvenance(records, audit); err != nil {
		t.Fatalf("valid provenance: %v", err)
	}
	bad := append([]Record(nil), records...)
	bad[0].Provenance &^= ProvenancePrismarine
	if err := validateRealProvenance(bad, audit); err == nil || !strings.Contains(err.Error(), "canonical provenance") {
		t.Fatalf("canonical provenance error = %v", err)
	}
	bad = append([]Record(nil), records...)
	bad[3].Provenance |= ProvenanceValentine
	if err := validateRealProvenance(bad, audit); err == nil || !strings.Contains(err.Error(), "Valentine provenance") {
		t.Fatalf("Valentine provenance error = %v", err)
	}
}

func TestNonCubeFamiliesOverrideLegacySolidFlags(t *testing.T) {
	nonUnit := CollisionSeed{ShapeID: 2, Confidence: CollisionConfidenceCollisionOnly, Boxes: []CollisionBox{{MaxX: 100_000_000, MaxY: 90_000_000, MaxZ: 100_000_000}}}
	for _, family := range []ModelFamily{ModelFamilyStatue, ModelFamilyCuboid} {
		record := Record{ModelFamily: family, Flags: flagCubeGeometry | flagOccludesFullFace, CollisionSeed: nonUnit}
		finalizeGeometryFacts(&record)
		if record.Flags != 0 || record.FaceCoverage != 0 || record.ModelFamily != family {
			t.Fatalf("family %v retained false cube facts: %+v", family, record)
		}
	}
	unit := CollisionSeed{ShapeID: 1, Confidence: CollisionConfidenceCollisionOnly, Boxes: []CollisionBox{{MaxX: 100_000_000, MaxY: 100_000_000, MaxZ: 100_000_000}}}
	record := Record{ModelFamily: ModelFamilyCube, CollisionSeed: unit}
	finalizeGeometryFacts(&record)
	if record.FaceCoverage != 0x3f {
		t.Fatalf("reviewed split cube coverage = %#x", record.FaceCoverage)
	}
}

func TestRequiredFamilySelectorFlags(t *testing.T) {
	tests := []struct {
		state       SourceState
		orientation uint32
		flags       uint32
	}{
		{sourceState("minecraft:golden_rail", intState("rail_direction", 5), byteState("rail_data_bit", 1)), 5, modelFlagPowered},
		{sourceState("minecraft:stone_button", intState("facing_direction", 3), byteState("button_pressed_bit", 1)), 3, modelFlagPressed},
		{sourceState("minecraft:lever", StateProperty{Name: "lever_direction", Value: TypedScalar{Kind: ScalarString, String: "up_east_west"}}), 6, 0},
		{sourceState("minecraft:oak_hanging_sign", intState("ground_sign_direction", 7), byteState("attached_bit", 1), byteState("hanging", 1)), 7, modelFlagAttached | modelFlagHanging},
		{sourceState("minecraft:red_bed", intState("direction", 2), byteState("head_piece_bit", 1), byteState("occupied_bit", 1)), 2, modelFlagHead | modelFlagOccupied},
		{sourceState("minecraft:oak_fence_gate", intState("direction", 1), byteState("in_wall_bit", 1)), 1, modelFlagInWall},
		{sourceState("minecraft:oak_door", intState("direction", 3), byteState("upper_block_bit", 1)), 3, modelFlagUpper},
		{sourceState("minecraft:test", StateProperty{Name: "cardinal_direction", Value: TypedScalar{Kind: ScalarString, String: "east"}}), 3, 0},
		{sourceState("minecraft:torch", StateProperty{Name: "torch_facing_direction", Value: TypedScalar{Kind: ScalarString, String: "south"}}), 4, 0},
	}
	for _, test := range tests {
		record, err := classifyRecord(test.state)
		if err != nil {
			t.Fatalf("classify %s: %v", test.state.Name, err)
		}
		if got, ok := record.ModelState.Get(ModelStateOrientation); !ok || got != test.orientation {
			t.Errorf("%s orientation = %d/%v, want %d", test.state.Name, got, ok, test.orientation)
		}
		if test.flags != 0 {
			if got, ok := record.ModelState.Get(ModelStateFlags); !ok || got != test.flags {
				t.Errorf("%s flags = %#x/%v, want %#x", test.state.Name, got, ok, test.flags)
			}
		}
	}
}

func TestPressurePlateRedstoneSignalBecomesTypedPressedFlag(t *testing.T) {
	for signal := int32(0); signal <= 15; signal++ {
		state := sourceState(
			"minecraft:wooden_pressure_plate",
			intState("redstone_signal", signal),
		)
		record, err := classifyRecord(state)
		if err != nil {
			t.Fatalf("classify signal %d: %v", signal, err)
		}
		if record.ModelFamily != ModelFamilyPressurePlate {
			t.Fatalf("signal %d family = %v", signal, record.ModelFamily)
		}
		want := uint32(0)
		if signal != 0 {
			want = modelFlagPressed
		}
		if got, ok := record.ModelState.Get(ModelStateFlags); !ok || got != want {
			t.Errorf("signal %d flags = %#x/%v, want %#x/present", signal, got, ok, want)
		}
	}
}

func TestRedstoneSignalSelectorIsPressurePlateScoped(t *testing.T) {
	record, err := classifyRecord(sourceState("minecraft:test", intState("redstone_signal", 15)))
	if err != nil {
		t.Fatalf("classify unrelated redstone state: %v", err)
	}
	if got, ok := record.ModelState.Get(ModelStateFlags); ok {
		t.Fatalf("unrelated redstone signal leaked typed flags %#x", got)
	}
}

func TestPressurePlateRejectsRedstoneSignalOutsideTypedDomain(t *testing.T) {
	for _, signal := range []int32{-1, 16} {
		_, err := classifyRecord(sourceState(
			"minecraft:wooden_pressure_plate",
			intState("redstone_signal", signal),
		))
		if err == nil {
			t.Fatalf("signal %d unexpectedly classified", signal)
		}
	}
}

func TestClassifyRecordCanonicalPropertyOrder(t *testing.T) {
	properties := []StateProperty{
		intState("direction", 1),
		intState("facing_direction", 2),
		byteState("upper_block_bit", 1),
	}
	forward, err := classifyRecord(sourceState("minecraft:test", properties...))
	if err != nil {
		t.Fatalf("classify forward: %v", err)
	}
	slices.Reverse(properties)
	reverse, err := classifyRecord(sourceState("minecraft:test", properties...))
	if err != nil {
		t.Fatalf("classify reverse: %v", err)
	}
	if forward.ModelState != reverse.ModelState || !bytes.Equal(forward.StateJSON, reverse.StateJSON) {
		t.Fatalf("classification depends on property order: %+v vs %+v", forward.ModelState, reverse.ModelState)
	}
}
