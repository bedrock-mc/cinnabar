package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/df-mc/dragonfly/server/world"
)

const retailBiomeCount = 87
const sourceBiomeCount = retailBiomeCount + 1
const excludedBiomeID = 194

type biomeCoverageManifest struct {
	Schema          uint32              `json:"schema"`
	Protocol        uint32              `json:"protocol"`
	ExpectedRecords uint32              `json:"expected_records"`
	AllowedIDRanges []sequentialIDRange `json:"allowed_id_ranges"`
}

func readBiomeCoverageManifest(path string) (map[uint32]struct{}, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read biome coverage manifest: %w", err)
	}
	if len(data) > 1<<20 {
		return nil, fmt.Errorf("biome coverage manifest exceeds 1 MiB")
	}
	var manifest biomeCoverageManifest
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&manifest); err != nil {
		return nil, fmt.Errorf("decode biome coverage manifest: %w", err)
	}
	if err := decoder.Decode(&struct{}{}); err != io.EOF {
		if err == nil {
			return nil, fmt.Errorf("decode biome coverage manifest: multiple JSON values")
		}
		return nil, fmt.Errorf("decode biome coverage manifest trailing data: %w", err)
	}
	if manifest.Schema != 1 || manifest.Protocol != registryProtocol || manifest.ExpectedRecords != retailBiomeCount {
		return nil, fmt.Errorf("biome coverage manifest header is not schema 1 protocol %d with %d records", registryProtocol, retailBiomeCount)
	}
	allowed := make(map[uint32]struct{}, retailBiomeCount)
	var previous uint32
	for index, span := range manifest.AllowedIDRanges {
		if span.first > span.last || span.last > mathMaxUint16 || (index != 0 && span.first <= previous) {
			return nil, fmt.Errorf("biome coverage range %d is invalid or out of order", index)
		}
		for id := span.first; id <= span.last; id++ {
			allowed[id] = struct{}{}
		}
		previous = span.last
	}
	if len(allowed) != retailBiomeCount {
		return nil, fmt.Errorf("biome coverage manifest contains %d IDs, want %d", len(allowed), retailBiomeCount)
	}
	if _, exists := allowed[excludedBiomeID]; exists {
		return nil, fmt.Errorf("biome coverage manifest includes excluded ID %d", excludedBiomeID)
	}
	return allowed, nil
}

const mathMaxUint16 = uint32(^uint16(0))

func projectRetailBiomes(source []BiomeRecord, allowed map[uint32]struct{}) ([]BiomeRecord, error) {
	if len(source) != sourceBiomeCount {
		return nil, fmt.Errorf("source biome count %d does not match %d", len(source), sourceBiomeCount)
	}
	byID := make(map[uint32]BiomeRecord, len(source))
	for _, record := range source {
		if _, exists := byID[record.ID]; exists {
			return nil, fmt.Errorf("duplicate source biome ID %d", record.ID)
		}
		if _, retained := allowed[record.ID]; !retained && record.ID != excludedBiomeID {
			return nil, fmt.Errorf("source biome ID %d is neither retained nor the reviewed exclusion", record.ID)
		}
		byID[record.ID] = record
	}
	if _, exists := byID[excludedBiomeID]; !exists {
		return nil, fmt.Errorf("source registry is missing reviewed excluded biome ID %d", excludedBiomeID)
	}
	projected := make([]BiomeRecord, 0, len(allowed))
	for id := range allowed {
		record, exists := byID[id]
		if !exists {
			return nil, fmt.Errorf("allowed biome ID %d is missing from the source registry", id)
		}
		projected = append(projected, record)
	}
	sort.Slice(projected, func(i, j int) bool { return projected[i].ID < projected[j].ID })
	if len(projected) != retailBiomeCount {
		return nil, fmt.Errorf("projected biome count %d does not match %d", len(projected), retailBiomeCount)
	}
	return projected, nil
}

func writeProjectedBiomeRegistry(output, coveragePath string) error {
	allowed, err := readBiomeCoverageManifest(coveragePath)
	if err != nil {
		return err
	}
	source, err := collectBiomes(world.Biomes())
	if err != nil {
		return err
	}
	projected, err := projectRetailBiomes(source, allowed)
	if err != nil {
		return err
	}
	encoded, err := encodeBiomeRegistry(projected)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(output), 0o755); err != nil {
		return fmt.Errorf("create biome output directory: %w", err)
	}
	if err := os.WriteFile(output, encoded, 0o644); err != nil {
		return fmt.Errorf("write biome output: %w", err)
	}
	digest := sha256.Sum256(encoded)
	shaPath := strings.TrimSuffix(output, filepath.Ext(output)) + ".sha256"
	if err := os.WriteFile(shaPath, []byte(fmt.Sprintf("%x\n", digest)), 0o644); err != nil {
		return fmt.Errorf("write biome checksum: %w", err)
	}
	return nil
}
