package main

import (
	"bytes"
	"math/rand"
	"strings"
	"testing"

	_ "github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/world"
)

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
	if len(records) < 1000 {
		t.Fatalf("registry too small: %d", len(records))
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
