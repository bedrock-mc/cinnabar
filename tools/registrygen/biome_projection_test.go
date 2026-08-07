package main

import (
	"crypto/sha256"
	"encoding/binary"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"testing"

	"github.com/df-mc/dragonfly/server/world"
)

func TestBiomeCoverageManifestIsSortedDefaultDenyAndExact(t *testing.T) {
	allowed, err := readBiomeCoverageManifest("../../assets/biome-coverage-v1001.json")
	if err != nil {
		t.Fatal(err)
	}
	if len(allowed) != retailBiomeCount {
		t.Fatalf("allowed biome count = %d, want %d", len(allowed), retailBiomeCount)
	}
	if _, ok := allowed[194]; ok {
		t.Fatal("non-retail biome ID is allowed")
	}
	source := make([]BiomeRecord, 0, sourceBiomeCount)
	for id := range allowed {
		source = append(source, BiomeRecord{ID: id, Name: fmt.Sprintf("example:biome_%d", id)})
	}
	source = append(source, BiomeRecord{ID: excludedBiomeID, Name: "example:excluded"})
	projected, err := projectRetailBiomes(source, allowed)
	if err != nil {
		t.Fatal(err)
	}
	if len(projected) != retailBiomeCount {
		t.Fatalf("projected biome count = %d, want %d", len(projected), retailBiomeCount)
	}
	if !slices.IsSortedFunc(projected, func(a, b BiomeRecord) int { return int(a.ID) - int(b.ID) }) {
		t.Fatal("projected biome records are not sorted")
	}
	for _, record := range projected {
		if _, ok := allowed[record.ID]; !ok {
			t.Fatalf("default-deny projection retained ID %d", record.ID)
		}
		want := fmt.Sprintf("example:biome_%d", record.ID)
		if record.Name != want {
			t.Fatalf("projected biome %d name = %q, want %q", record.ID, record.Name, want)
		}
	}
	missing := slices.DeleteFunc(source, func(record BiomeRecord) bool { return record.ID == 0 })
	if _, err := projectRetailBiomes(missing, allowed); err == nil {
		t.Fatal("missing allowed biome ID was accepted")
	}
	unexpected := slices.Clone(source)
	unexpected[len(unexpected)-1].ID = excludedBiomeID + 1
	if _, err := projectRetailBiomes(unexpected, allowed); err == nil {
		t.Fatal("unexpected source biome ID was accepted")
	}
}

func TestBiomeCoverageManifestRejectsTrailingJSON(t *testing.T) {
	data, err := os.ReadFile("../../assets/biome-coverage-v1001.json")
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(t.TempDir(), "coverage.json")
	if err := os.WriteFile(path, append(data, []byte("\n{}\n")...), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := readBiomeCoverageManifest(path); err == nil {
		t.Fatal("coverage manifest with trailing JSON was accepted")
	}
}

func TestCheckedInBiomeRegistryExactlyMatchesSourceProjection(t *testing.T) {
	allowed, err := readBiomeCoverageManifest("../../assets/biome-coverage-v1001.json")
	if err != nil {
		t.Fatal(err)
	}
	source, err := collectBiomes(world.Biomes())
	if err != nil {
		t.Fatal(err)
	}
	projected, err := projectRetailBiomes(source, allowed)
	if err != nil {
		t.Fatal(err)
	}
	want, err := encodeBiomeRegistry(projected)
	if err != nil {
		t.Fatal(err)
	}
	got, err := os.ReadFile("../../crates/assets/data/biome-registry-v1001.bin")
	if err != nil {
		t.Fatal(err)
	}
	if !slices.Equal(got, want) {
		t.Fatal("checked-in biome registry does not exactly match the retained source ID/name projection")
	}
}

func TestCheckedInBiomeRegistryIsProjectedAndHashBound(t *testing.T) {
	data, err := os.ReadFile("../../crates/assets/data/biome-registry-v1001.bin")
	if err != nil {
		t.Fatal(err)
	}
	if string(data[:8]) != biomeRegistryHeader {
		t.Fatal("unexpected biome registry header")
	}
	if got := binary.LittleEndian.Uint32(data[8:12]); got != retailBiomeCount {
		t.Fatalf("checked-in biome count = %d, want %d", got, retailBiomeCount)
	}
	digest := fmt.Sprintf("%x", sha256.Sum256(data))
	const expected = "1c5c567c38bad94f61f21b83f2848db151fcd07a44a3bdcc2aea0c8ae5f9b62c"
	if digest != expected {
		t.Fatalf("checked-in biome SHA-256 = %s, want %s", digest, expected)
	}
}
