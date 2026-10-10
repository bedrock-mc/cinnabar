package main

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"unicode/utf8"

	shared "github.com/bedrock-mc/protocolgen/generated/data"
	sharedbiome "github.com/bedrock-mc/protocolgen/generated/data/biome"
)

const (
	v2193RetailBiomeCount = 89
	v2193BiomeAllowSHA256 = "6127c74c17455273bb5226f1e05e98709bc247c05a0137a8827cb97756c3b198"

	v2193BiomeOutputPath     = "crates/assets/data/biome-registry-v2193.bin"
	v2193BiomeAllowlistPath  = "crates/protocol/data/retail_biomes_1_26_50.txt"
	v2193BiomeProjectionPath = "assets/biome-projection-v2193.json"
)

type v2193BiomeProjectionStats struct {
	SourceCount        int
	IgnoredCount       int
	IgnoredFingerprint string
}

type v2193BiomeProjectionManifest struct {
	Schema      string                      `json:"schema"`
	GameVersion string                      `json:"game_version"`
	Protocol    uint32                      `json:"protocol"`
	Sources     v2193BiomeProjectionSources `json:"sources"`
	Allowlist   v2193BiomeProjectionAllow   `json:"allowlist"`
	Projection  v2193BiomeProjectionSummary `json:"projection"`
	Output      v2193BiomeProjectionOutput  `json:"output"`
}

type v2193BiomeProjectionSources struct {
	SharedCatalog v2193BiomeCatalogSource `json:"shared_catalog"`
}

type v2193BiomeCatalogSource struct {
	Module           string `json:"module"`
	SourceLockSHA256 string `json:"source_lock_sha256"`
	BiomeCount       int    `json:"biome_count"`
}

type v2193BiomeProjectionAllow struct {
	Path   string `json:"path"`
	SHA256 string `json:"sha256"`
	Count  int    `json:"count"`
}

type v2193BiomeProjectionSummary struct {
	Retained           int    `json:"retained"`
	IgnoredCount       int    `json:"ignored_count"`
	IgnoredFingerprint string `json:"ignored_fingerprint"`
}

type v2193BiomeProjectionOutput struct {
	Format string `json:"format"`
	Path   string `json:"path"`
	SHA256 string `json:"sha256"`
}

// validateSharedBiomeTarget binds numeric IDs to this carrier's release and source lock.
func validateSharedBiomeTarget() error {
	if shared.MinecraftVersion != v2193GameVersion || shared.ProtocolVersion != v2193BlockProtocol {
		return fmt.Errorf("shared biome catalog target %s/%d does not match carrier %s/%d", shared.MinecraftVersion, shared.ProtocolVersion, v2193GameVersion, v2193BlockProtocol)
	}
	if shared.SourceLockSHA256 != shared.SemanticSourceLockSHA256 {
		return errors.New("shared biome catalog and release have different source locks")
	}
	if len(sharedbiome.All()) != int(shared.GeneratedCounts.Biomes) {
		return errors.New("shared biome catalog count does not match its metadata")
	}
	return nil
}

// parseV2193BiomeAllowlist accepts only the reviewed, sorted retail name set.
func parseV2193BiomeAllowlist(data []byte) (map[string]struct{}, error) {
	if digest := fmt.Sprintf("%x", sha256.Sum256(data)); digest != v2193BiomeAllowSHA256 {
		return nil, fmt.Errorf("v2193 biome allowlist SHA-256 %s does not match pinned identity", digest)
	}
	lines := strings.Split(strings.TrimSuffix(string(data), "\n"), "\n")
	if len(lines) != v2193RetailBiomeCount {
		return nil, fmt.Errorf("v2193 biome allowlist contains %d names, want %d", len(lines), v2193RetailBiomeCount)
	}
	allowed := make(map[string]struct{}, len(lines))
	previous := ""
	for index, raw := range lines {
		name := strings.TrimSuffix(raw, "\r")
		if !strings.HasPrefix(name, "minecraft:") || len(name) > maxBiomeNameBytes || (index != 0 && name <= previous) {
			return nil, fmt.Errorf("v2193 biome allowlist is invalid at entry %d", index)
		}
		allowed[name] = struct{}{}
		previous = name
	}
	return allowed, nil
}

// readV2193BiomeAllowlist loads the local admission policy without changing shared facts.
func readV2193BiomeAllowlist(path string) (map[string]struct{}, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read v2193 biome allowlist: %w", err)
	}
	return parseV2193BiomeAllowlist(data)
}

// projectV2193BiomeRecords keeps allowlisted shared IDs and fingerprints excluded records.
func projectV2193BiomeRecords(source []sharedbiome.Biome, allowed map[string]struct{}) ([]BiomeRecord, v2193BiomeProjectionStats, error) {
	stats := v2193BiomeProjectionStats{SourceCount: len(source)}
	if len(source) > maxBiomeRecordCount || len(allowed) != v2193RetailBiomeCount {
		return nil, stats, errors.New("v2193 biome source or allowlist count is outside the carrier scope")
	}
	retained := make([]BiomeRecord, 0, len(allowed))
	ignored := make([]BiomeRecord, 0)
	seenIDs := make(map[int32]struct{}, len(source))
	seenNames := make(map[string]struct{}, len(source))
	for _, definition := range source {
		if !definition.HasID || definition.ID < 0 || definition.ID > math.MaxUint16 {
			return nil, stats, fmt.Errorf("shared biome %q has no valid uint16 ID", definition.Name)
		}
		if !strings.HasPrefix(definition.Name, "minecraft:") || len(definition.Name) > maxBiomeNameBytes || !utf8.ValidString(definition.Name) {
			return nil, stats, fmt.Errorf("shared biome ID %d has an invalid name", definition.ID)
		}
		if _, exists := seenIDs[definition.ID]; exists {
			return nil, stats, fmt.Errorf("duplicate shared biome ID %d", definition.ID)
		}
		if _, exists := seenNames[definition.Name]; exists {
			return nil, stats, fmt.Errorf("duplicate shared biome name %q", definition.Name)
		}
		seenIDs[definition.ID], seenNames[definition.Name] = struct{}{}, struct{}{}
		record := BiomeRecord{ID: uint32(definition.ID), Name: definition.Name}
		if _, keep := allowed[record.Name]; keep {
			retained = append(retained, record)
		} else {
			ignored = append(ignored, record)
		}
	}
	if len(retained) != len(allowed) {
		return nil, stats, fmt.Errorf("v2193 biome projection is missing %d retained names", len(allowed)-len(retained))
	}
	sort.Slice(retained, func(i, j int) bool { return retained[i].ID < retained[j].ID })
	sort.Slice(ignored, func(i, j int) bool { return ignored[i].ID < ignored[j].ID })
	fingerprint := sha256.New()
	for _, record := range ignored {
		_ = binary.Write(fingerprint, binary.LittleEndian, record.ID)
		_ = binary.Write(fingerprint, binary.LittleEndian, uint16(len(record.Name)))
		_, _ = fingerprint.Write([]byte(record.Name))
	}
	stats.IgnoredCount = len(ignored)
	stats.IgnoredFingerprint = fmt.Sprintf("%x", fingerprint.Sum(nil))
	return retained, stats, nil
}

// encodeV2193BiomeProjection writes the existing binary format and shared source identity.
func encodeV2193BiomeProjection(records []BiomeRecord, stats v2193BiomeProjectionStats) ([]byte, []byte, error) {
	carrier, err := encodeBiomeRegistry(records)
	if err != nil {
		return nil, nil, err
	}
	if len(records) != v2193RetailBiomeCount || stats.IgnoredCount < 0 || stats.SourceCount != len(records)+stats.IgnoredCount || len(stats.IgnoredFingerprint) != 64 {
		return nil, nil, errors.New("v2193 biome projection metadata is incomplete")
	}
	manifest := v2193BiomeProjectionManifest{
		Schema: "cinnabar.biome-projection.v3", GameVersion: shared.MinecraftVersion, Protocol: shared.ProtocolVersion,
		Sources: v2193BiomeProjectionSources{SharedCatalog: v2193BiomeCatalogSource{
			Module: "github.com/bedrock-mc/protocolgen/generated/data", SourceLockSHA256: shared.SourceLockSHA256, BiomeCount: stats.SourceCount,
		}},
		Allowlist:  v2193BiomeProjectionAllow{Path: v2193BiomeAllowlistPath, SHA256: v2193BiomeAllowSHA256, Count: v2193RetailBiomeCount},
		Projection: v2193BiomeProjectionSummary{Retained: len(records), IgnoredCount: stats.IgnoredCount, IgnoredFingerprint: stats.IgnoredFingerprint},
		Output:     v2193BiomeProjectionOutput{Format: biomeRegistryHeader, Path: v2193BiomeOutputPath, SHA256: fmt.Sprintf("%x", sha256.Sum256(carrier))},
	}
	manifestBytes, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil {
		return nil, nil, fmt.Errorf("encode v2193 biome manifest: %w", err)
	}
	return carrier, append(manifestBytes, '\n'), nil
}

// writeV2193BiomeProjection projects shared IDs into Cinnabar's allowlisted biome carrier.
func writeV2193BiomeProjection(allowlistPath, outputPath, manifestPath string) error {
	if err := validateSharedBiomeTarget(); err != nil {
		return err
	}
	allowed, err := readV2193BiomeAllowlist(allowlistPath)
	if err != nil {
		return err
	}
	projected, stats, err := projectV2193BiomeRecords(sharedbiome.All(), allowed)
	if err != nil {
		return err
	}
	carrier, manifest, err := encodeV2193BiomeProjection(projected, stats)
	if err != nil {
		return err
	}
	for _, path := range []string{outputPath, manifestPath} {
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			return fmt.Errorf("create v2193 biome output directory: %w", err)
		}
	}
	if err := os.WriteFile(outputPath, carrier, 0o644); err != nil {
		return fmt.Errorf("write v2193 biome registry: %w", err)
	}
	shaPath := strings.TrimSuffix(outputPath, filepath.Ext(outputPath)) + ".sha256"
	if err := os.WriteFile(shaPath, []byte(fmt.Sprintf("%x\n", sha256.Sum256(carrier))), 0o644); err != nil {
		return fmt.Errorf("write v2193 biome checksum: %w", err)
	}
	if err := os.WriteFile(manifestPath, manifest, 0o644); err != nil {
		return fmt.Errorf("write v2193 biome manifest: %w", err)
	}
	return nil
}
