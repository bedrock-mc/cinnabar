package main

import (
	"bytes"
	"math"
	"math/rand"
	"strings"
	"testing"

	_ "github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/model"
	"github.com/df-mc/dragonfly/server/world"
)

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
			Flags:        flagFullCube,
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
		'B', 'R', 'E', 'G', '1', '0', '0', '1',
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
		0x02,
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

func TestFullCubeClassificationPreservesSolidAndFailsClosedOverrides(t *testing.T) {
	tests := []struct {
		name  string
		block classifierBlock
		want  bool
	}{
		{
			name:  "implemented solid",
			block: classifierBlock{name: "minecraft:stone", stateHash: 1, model: model.Solid{}},
			want:  true,
		},
		{
			name:  "approved unknown",
			block: classifierBlock{name: "minecraft:mycelium", stateHash: math.MaxUint64, model: model.Empty{}},
			want:  true,
		},
		{
			name:  "implemented non-solid target",
			block: classifierBlock{name: "minecraft:mycelium", stateHash: 1, model: model.Empty{}},
			want:  false,
		},
		{
			name:  "unapproved unknown",
			block: classifierBlock{name: "minecraft:acacia_sapling", stateHash: math.MaxUint64, model: model.Empty{}},
			want:  false,
		},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if got := fullCube(test.block); got != test.want {
				t.Fatalf("fullCube() = %v, want %v", got, test.want)
			}
		})
	}
}

func TestEncodeIsStableAcrossShuffledInputs(t *testing.T) {
	records := make([]Record, 16)
	for i := range records {
		records[i] = Record{
			SequentialID: uint32(i),
			NetworkHash:  uint32(10_000 + i),
			Flags:        uint8(i % 4),
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
	if air.Flags&flagAir == 0 {
		t.Fatal("air flag missing")
	}
	if air.NetworkHash != 0xdbf44120 {
		t.Fatalf("air hash = %#x", air.NetworkHash)
	}
	stone := findByName(t, records, "minecraft:stone")
	if stone.Flags&flagFullCube == 0 {
		t.Fatal("stone full-cube flag missing")
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
			if record.Flags&flagFullCube == 0 {
				t.Errorf("%s state %s is missing the full-cube flag", name, record.StateJSON)
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
			if record.Flags&flagFullCube != 0 {
				t.Errorf("negative-control block %s state %s was marked full cube", name, record.StateJSON)
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
