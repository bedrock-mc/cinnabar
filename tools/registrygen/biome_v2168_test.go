package main

import (
	"bytes"
	"encoding/binary"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"testing"
)

func TestV2168BiomeTableRejectsMalformedDuplicateRangeAndTrailingData(t *testing.T) {
	table, names := syntheticV2168BiomeTable(t)
	resolve := syntheticBiomeResolver(names)
	tests := []struct {
		name    string
		mutate  func([]byte)
		input   func([]byte) []byte
		wantErr string
	}{
		{name: "malformed name pointer", mutate: func(data []byte) { binary.LittleEndian.PutUint64(data[8:16], 1) }, wantErr: "resolve name"},
		{name: "duplicate id", mutate: func(data []byte) { copy(data[24:32], data[0:8]) }, wantErr: "duplicate biome ID"},
		{name: "duplicate name", mutate: func(data []byte) { copy(data[32:48], data[8:24]) }, wantErr: "duplicate biome name"},
		{name: "id range", mutate: func(data []byte) { binary.LittleEndian.PutUint64(data[0:8], 1<<16) }, wantErr: "outside"},
		{name: "trailing record data", input: func(data []byte) []byte { return append(data, 0) }, wantErr: "table size"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			input := append([]byte(nil), table...)
			if test.mutate != nil {
				test.mutate(input)
			}
			if test.input != nil {
				input = test.input(input)
			}
			_, err := parseV2168BiomeRecords(input, resolve)
			if err == nil || !strings.Contains(err.Error(), test.wantErr) {
				t.Fatalf("error = %v, want %q", err, test.wantErr)
			}
		})
	}
}

func TestV2168BiomeProjectionIsDefaultDenyAndRejectsMissingOrExtraScope(t *testing.T) {
	records, allowed, pmmp, dragonfly := syntheticV2168Projection()
	projected, stats, err := projectV2168BiomeRecords(records, allowed, pmmp, dragonfly)
	if err != nil {
		t.Fatal(err)
	}
	if len(projected) != v2168RetailBiomeCount || stats.IgnoredCount != 1 || len(stats.IgnoredFingerprint) != 64 {
		t.Fatalf("projection=%d ignored=%d fingerprint=%q", len(projected), stats.IgnoredCount, stats.IgnoredFingerprint)
	}
	if !sort.SliceIsSorted(projected, func(i, j int) bool { return projected[i].ID < projected[j].ID }) {
		t.Fatal("projection is not sorted by numeric ID")
	}

	missing := append([]BiomeRecord(nil), records...)
	missing[0].Name = "example:second_ignored"
	if _, _, err := projectV2168BiomeRecords(missing, allowed, pmmp, dragonfly); err == nil || !strings.Contains(err.Error(), "missing") {
		t.Fatalf("missing retained name error = %v", err)
	}
	extra := append([]BiomeRecord(nil), records...)
	extra[len(extra)-1].Name = extra[0].Name
	if _, _, err := projectV2168BiomeRecords(extra, allowed, pmmp, dragonfly); err == nil {
		t.Fatal("accepted scope record that entered the allowlist")
	}
	pmmp[records[0].Name]++
	if _, _, err := projectV2168BiomeRecords(records, allowed, pmmp, dragonfly); err == nil || !strings.Contains(err.Error(), "PMMP") {
		t.Fatalf("PMMP mismatch error = %v", err)
	}
}

func TestV2168BiomeInputsRejectSourceHashAllowlistAndTrailingPMMPJSON(t *testing.T) {
	path := filepath.Join(t.TempDir(), "source.bin")
	if err := os.WriteFile(path, []byte("drift"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := verifyV2168FileSHA256(path, v2168BDSExecutableSHA256); err == nil || !strings.Contains(err.Error(), "SHA-256") {
		t.Fatalf("source hash error = %v", err)
	}
	if _, err := parseV2168BiomeAllowlist([]byte("minecraft:only_one\n")); err == nil {
		t.Fatal("accepted incomplete allowlist")
	}
	if _, err := decodeV2168PMMPBiomeMap([]byte(`{"test":1} {}`)); err == nil || !strings.Contains(err.Error(), "trailing") {
		t.Fatalf("trailing PMMP JSON error = %v", err)
	}
}

func TestV2168BiomeProjectionEncodingIsTwoRunIdentical(t *testing.T) {
	records, allowed, pmmp, dragonfly := syntheticV2168Projection()
	projected, stats, err := projectV2168BiomeRecords(records, allowed, pmmp, dragonfly)
	if err != nil {
		t.Fatal(err)
	}
	firstCarrier, firstManifest, err := encodeV2168BiomeProjection(projected, stats)
	if err != nil {
		t.Fatal(err)
	}
	secondCarrier, secondManifest, err := encodeV2168BiomeProjection(projected, stats)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(firstCarrier, secondCarrier) || !bytes.Equal(firstManifest, secondManifest) {
		t.Fatal("identical v2168 biome generations differed")
	}
}

func syntheticV2168BiomeTable(t *testing.T) ([]byte, map[uint64][]byte) {
	t.Helper()
	records, _, _, _ := syntheticV2168Projection()
	table := make([]byte, len(records)*v2168BiomeRecordSize)
	names := make(map[uint64][]byte, len(records))
	for index, record := range records {
		nameVA := uint64(0x140100000 + index*0x100)
		start := index * v2168BiomeRecordSize
		binary.LittleEndian.PutUint64(table[start:start+8], uint64(record.ID))
		binary.LittleEndian.PutUint64(table[start+8:start+16], nameVA)
		binary.LittleEndian.PutUint64(table[start+16:start+24], uint64(len(record.Name)))
		names[nameVA] = []byte(record.Name)
	}
	return table, names
}

func syntheticBiomeResolver(names map[uint64][]byte) func(uint64, uint64) ([]byte, error) {
	return func(address, length uint64) ([]byte, error) {
		name, ok := names[address]
		if !ok || uint64(len(name)) != length {
			return nil, fmt.Errorf("unmapped name")
		}
		return append([]byte(nil), name...), nil
	}
}

func syntheticV2168Projection() ([]BiomeRecord, map[string]struct{}, map[string]uint32, map[string]uint32) {
	records := make([]BiomeRecord, 0, v2168BiomeSourceCount)
	allowed := make(map[string]struct{}, v2168RetailBiomeCount)
	pmmp := make(map[string]uint32, v2168RetailBiomeCount)
	dragonfly := make(map[string]uint32, v2168RetailBiomeCount)
	for index := range v2168RetailBiomeCount {
		name := fmt.Sprintf("minecraft:test_%03d", index)
		id := uint32((index*37 + 11) % 194)
		for mapContainsValue(pmmp, id) {
			id = (id + 1) % 194
		}
		record := BiomeRecord{ID: id, Name: name}
		records = append(records, record)
		allowed[name], pmmp[name], dragonfly[name] = struct{}{}, id, id
	}
	records = append(records, BiomeRecord{ID: 195, Name: "minecraft:ignored_test"})
	return records, allowed, pmmp, dragonfly
}

func mapContainsValue(values map[string]uint32, want uint32) bool {
	for _, value := range values {
		if value == want {
			return true
		}
	}
	return false
}
