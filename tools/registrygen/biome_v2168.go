package main

import (
	"bytes"
	"crypto/sha256"
	"debug/pe"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"unicode/utf8"

	"github.com/df-mc/dragonfly/server/world"
)

const (
	v2168BiomeRecordSize       = 24
	v2168BiomeSourceCount      = 89
	v2168RetailBiomeCount      = 88
	v2168BiomeTableOffset      = 0x0a62_1678
	v2168BiomeRetailTableBytes = v2168RetailBiomeCount * v2168BiomeRecordSize
	v2168BiomeTableBytes       = v2168BiomeSourceCount * v2168BiomeRecordSize
	v2168ImageBase             = 0x1_4000_0000

	v2168BDSExecutableSHA256 = "e7775e636b9fdcbc354823d92d0c22c12738a2141d12557d856744293d258372"
	v2168BiomeTableSHA256    = "bb2bf873e15abbb8c706d318ae4332072c323a70d4bb3637ceef456f8cbd161b"
	v2168RetailTableSHA256   = "2db76542d7986789bf44fea1013676f08882c121ae737ccec2e3407be9acabc9"
	v2168PMMPBiomeMapSHA256  = "4f27df3f1e58476fc65e337f7cf3e275f65a98b6c40ea46c31b24016b85e0052"
	v2168BiomeAllowSHA256    = "df7e18c18e939e21f387838479ee9c79b0d7eb798fb1bd906f51c56963058574"

	v2168BiomeOutputPath     = "crates/assets/data/biome-registry-v2168.bin"
	v2168BiomeAllowlistPath  = "crates/protocol/data/retail_biomes_1_26_40.txt"
	v2168BiomeProjectionPath = "assets/biome-projection-v2168.json"
)

type v2168BiomeProjectionStats struct {
	IgnoredCount       int
	IgnoredFingerprint string
}

type v2168BiomeProjectionManifest struct {
	Schema      string                      `json:"schema"`
	GameVersion string                      `json:"game_version"`
	Protocol    uint32                      `json:"protocol"`
	Sources     v2168BiomeProjectionSources `json:"sources"`
	Allowlist   v2168BiomeProjectionAllow   `json:"allowlist"`
	Projection  v2168BiomeProjectionSummary `json:"projection"`
	Output      v2168BiomeProjectionOutput  `json:"output"`
}

type v2168BiomeProjectionSources struct {
	BDS       v2168BiomeBDSSource       `json:"bds"`
	PMMP      v2168BiomePMMPSource      `json:"pmmp"`
	Dragonfly v2168BiomeDragonflySource `json:"dragonfly"`
	Retail    v2168BiomeRetailSource    `json:"retail"`
}

type v2168BiomeBDSSource struct {
	ExecutableSHA256 string `json:"executable_sha256"`
	TableOffset      uint64 `json:"table_offset"`
	TableRecords     int    `json:"table_records"`
	TableSHA256      string `json:"table_sha256"`
	RetailRecords    int    `json:"retail_records"`
	RetailSHA256     string `json:"retail_sha256"`
}

type v2168BiomePMMPSource struct {
	Commit string `json:"commit"`
	SHA256 string `json:"sha256"`
}

type v2168BiomeDragonflySource struct {
	Module    string `json:"module"`
	Version   string `json:"version"`
	ModuleSum string `json:"module_sum"`
}

type v2168BiomeRetailSource struct {
	PackageSHA256   string `json:"package_sha256"`
	BiomeFileSHA256 string `json:"biome_file_sha256"`
	BDSFileSHA256   string `json:"bds_file_sha256"`
}

type v2168BiomeProjectionAllow struct {
	Path   string `json:"path"`
	SHA256 string `json:"sha256"`
	Count  int    `json:"count"`
}

type v2168BiomeProjectionSummary struct {
	Retained           int    `json:"retained"`
	IgnoredCount       int    `json:"ignored_count"`
	IgnoredFingerprint string `json:"ignored_fingerprint"`
}

type v2168BiomeProjectionOutput struct {
	Format string `json:"format"`
	Path   string `json:"path"`
	SHA256 string `json:"sha256"`
}

func verifyV2168FileSHA256(path, expected string) error {
	file, err := os.Open(path)
	if err != nil {
		return fmt.Errorf("open source: %w", err)
	}
	hash := sha256.New()
	_, copyErr := io.Copy(hash, file)
	closeErr := file.Close()
	if err := errors.Join(copyErr, closeErr); err != nil {
		return fmt.Errorf("hash source: %w", err)
	}
	actual := fmt.Sprintf("%x", hash.Sum(nil))
	if actual != expected {
		return fmt.Errorf("source SHA-256 %s does not match pinned identity", actual)
	}
	return nil
}

func readV2168BDSBiomeRecords(path string) ([]BiomeRecord, error) {
	if err := verifyV2168FileSHA256(path, v2168BDSExecutableSHA256); err != nil {
		return nil, err
	}
	file, err := os.Open(path)
	if err != nil {
		return nil, fmt.Errorf("open pinned BDS executable: %w", err)
	}
	defer file.Close()
	image, err := pe.NewFile(file)
	if err != nil {
		return nil, fmt.Errorf("parse pinned BDS PE: %w", err)
	}
	if image.Machine != pe.IMAGE_FILE_MACHINE_AMD64 {
		return nil, fmt.Errorf("pinned BDS PE machine %#x is not AMD64", image.Machine)
	}
	optional, ok := image.OptionalHeader.(*pe.OptionalHeader64)
	if !ok || optional.ImageBase != v2168ImageBase {
		return nil, errors.New("pinned BDS PE has an unexpected image base")
	}
	table := make([]byte, v2168BiomeTableBytes)
	if _, err := file.ReadAt(table, v2168BiomeTableOffset); err != nil {
		return nil, fmt.Errorf("read pinned BDS biome table: %w", err)
	}
	if digest := fmt.Sprintf("%x", sha256.Sum256(table)); digest != v2168BiomeTableSHA256 {
		return nil, fmt.Errorf("pinned BDS biome table SHA-256 %s does not match", digest)
	}
	if digest := fmt.Sprintf("%x", sha256.Sum256(table[:v2168BiomeRetailTableBytes])); digest != v2168RetailTableSHA256 {
		return nil, fmt.Errorf("pinned BDS retail biome table SHA-256 %s does not match", digest)
	}
	resolve := func(address, length uint64) ([]byte, error) {
		offset, err := peVirtualAddressToFileOffset(image, address, length)
		if err != nil {
			return nil, err
		}
		name := make([]byte, int(length))
		if _, err := file.ReadAt(name, int64(offset)); err != nil {
			return nil, err
		}
		return name, nil
	}
	return parseV2168BiomeRecords(table, resolve)
}

func peVirtualAddressToFileOffset(image *pe.File, address, length uint64) (uint64, error) {
	optional, ok := image.OptionalHeader.(*pe.OptionalHeader64)
	if !ok || address < optional.ImageBase {
		return 0, errors.New("virtual address is outside the PE image")
	}
	rva := address - optional.ImageBase
	for _, section := range image.Sections {
		start := uint64(section.VirtualAddress)
		rawSize := uint64(section.Size)
		if rva < start || rva-start > rawSize || length > rawSize-(rva-start) {
			continue
		}
		return uint64(section.Offset) + (rva - start), nil
	}
	return 0, errors.New("virtual address does not map to PE section data")
}

func parseV2168BiomeRecords(table []byte, resolve func(uint64, uint64) ([]byte, error)) ([]BiomeRecord, error) {
	if len(table) != v2168BiomeTableBytes {
		return nil, fmt.Errorf("v2168 biome table size %d does not match %d", len(table), v2168BiomeTableBytes)
	}
	records := make([]BiomeRecord, 0, v2168BiomeSourceCount)
	seenIDs := make(map[uint32]struct{}, v2168BiomeSourceCount)
	seenNames := make(map[string]struct{}, v2168BiomeSourceCount)
	for index := range v2168BiomeSourceCount {
		start := index * v2168BiomeRecordSize
		id64 := binary.LittleEndian.Uint64(table[start : start+8])
		address := binary.LittleEndian.Uint64(table[start+8 : start+16])
		length := binary.LittleEndian.Uint64(table[start+16 : start+24])
		if id64 > uint64(^uint16(0)) {
			return nil, fmt.Errorf("v2168 biome record %d ID %d is outside uint16", index, id64)
		}
		if length == 0 || length > maxBiomeNameBytes {
			return nil, fmt.Errorf("v2168 biome record %d name length %d is outside bounds", index, length)
		}
		nameBytes, err := resolve(address, length)
		if err != nil {
			return nil, fmt.Errorf("resolve name for v2168 biome record %d: %w", index, err)
		}
		if uint64(len(nameBytes)) != length || !utf8.Valid(nameBytes) {
			return nil, fmt.Errorf("v2168 biome record %d has a malformed name", index)
		}
		name := string(nameBytes)
		if !strings.HasPrefix(name, "minecraft:") {
			return nil, fmt.Errorf("v2168 biome record %d lacks the required namespace prefix", index)
		}
		id := uint32(id64)
		if _, exists := seenIDs[id]; exists {
			return nil, fmt.Errorf("duplicate biome ID %d", id)
		}
		if _, exists := seenNames[name]; exists {
			return nil, fmt.Errorf("duplicate biome name at record %d", index)
		}
		seenIDs[id], seenNames[name] = struct{}{}, struct{}{}
		records = append(records, BiomeRecord{ID: id, Name: name})
	}
	return records, nil
}

func parseV2168BiomeAllowlist(data []byte) (map[string]struct{}, error) {
	if digest := fmt.Sprintf("%x", sha256.Sum256(data)); digest != v2168BiomeAllowSHA256 {
		return nil, fmt.Errorf("v2168 biome allowlist SHA-256 %s does not match pinned identity", digest)
	}
	lines := strings.Split(strings.TrimSuffix(string(data), "\n"), "\n")
	if len(lines) != v2168RetailBiomeCount {
		return nil, fmt.Errorf("v2168 biome allowlist contains %d names, want %d", len(lines), v2168RetailBiomeCount)
	}
	allowed := make(map[string]struct{}, len(lines))
	previous := ""
	for index, raw := range lines {
		name := strings.TrimSuffix(raw, "\r")
		if !strings.HasPrefix(name, "minecraft:") || len(name) > maxBiomeNameBytes || (index != 0 && name <= previous) {
			return nil, fmt.Errorf("v2168 biome allowlist is invalid at entry %d", index)
		}
		allowed[name] = struct{}{}
		previous = name
	}
	return allowed, nil
}

func readV2168BiomeAllowlist(path string) (map[string]struct{}, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read v2168 biome allowlist: %w", err)
	}
	return parseV2168BiomeAllowlist(data)
}

func decodeV2168PMMPBiomeMap(data []byte) (map[string]uint32, error) {
	decoder := json.NewDecoder(bytes.NewReader(data))
	var raw map[string]uint64
	if err := decoder.Decode(&raw); err != nil {
		return nil, fmt.Errorf("decode PMMP biome map: %w", err)
	}
	var trailing any
	if err := decoder.Decode(&trailing); !errors.Is(err, io.EOF) {
		return nil, errors.New("PMMP biome map has trailing JSON")
	}
	if digest := fmt.Sprintf("%x", sha256.Sum256(data)); digest != v2168PMMPBiomeMapSHA256 {
		return nil, fmt.Errorf("PMMP biome map SHA-256 %s does not match pinned identity", digest)
	}
	if len(raw) != v2168RetailBiomeCount {
		return nil, fmt.Errorf("PMMP biome map contains %d records, want %d", len(raw), v2168RetailBiomeCount)
	}
	result := make(map[string]uint32, len(raw))
	for rawName, id := range raw {
		name := canonicalBiomeName(rawName)
		if !strings.HasPrefix(name, "minecraft:") || id > uint64(^uint16(0)) {
			return nil, errors.New("PMMP biome map contains an invalid record")
		}
		if _, exists := result[name]; exists {
			return nil, errors.New("PMMP biome map contains a duplicate canonical name")
		}
		result[name] = uint32(id)
	}
	return result, nil
}

func readV2168PMMPBiomeMap(path string) (map[string]uint32, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read PMMP biome map: %w", err)
	}
	return decodeV2168PMMPBiomeMap(data)
}

func v2168DragonflyBiomeMap() (map[string]uint32, error) {
	records, err := collectBiomes(world.Biomes())
	if err != nil {
		return nil, err
	}
	if len(records) != v2168RetailBiomeCount {
		return nil, fmt.Errorf("Dragonfly biome map contains %d records, want %d", len(records), v2168RetailBiomeCount)
	}
	result := make(map[string]uint32, len(records))
	for _, record := range records {
		if _, exists := result[record.Name]; exists {
			return nil, fmt.Errorf("Dragonfly biome map duplicates %q", record.Name)
		}
		result[record.Name] = record.ID
	}
	return result, nil
}

func projectV2168BiomeRecords(source []BiomeRecord, allowed map[string]struct{}, pmmp, dragonfly map[string]uint32) ([]BiomeRecord, v2168BiomeProjectionStats, error) {
	stats := v2168BiomeProjectionStats{}
	if len(source) != v2168BiomeSourceCount || len(allowed) != v2168RetailBiomeCount {
		return nil, stats, errors.New("v2168 biome source or allowlist count does not match the pinned scope")
	}
	retained := make(map[string]uint32, v2168RetailBiomeCount)
	ignored := make([]BiomeRecord, 0, 1)
	for _, record := range source {
		if _, keep := allowed[record.Name]; keep {
			retained[record.Name] = record.ID
		} else {
			ignored = append(ignored, record)
		}
	}
	if len(retained) != v2168RetailBiomeCount {
		return nil, stats, fmt.Errorf("v2168 biome projection is missing %d retained names", v2168RetailBiomeCount-len(retained))
	}
	if len(ignored) != 1 {
		return nil, stats, fmt.Errorf("v2168 biome projection ignored %d records, want 1", len(ignored))
	}
	if err := compareV2168BiomeMap("PMMP", retained, pmmp); err != nil {
		return nil, stats, err
	}
	if err := compareV2168BiomeMap("Dragonfly", retained, dragonfly); err != nil {
		return nil, stats, err
	}
	projected := make([]BiomeRecord, 0, len(retained))
	for name, id := range retained {
		projected = append(projected, BiomeRecord{ID: id, Name: name})
	}
	sort.Slice(projected, func(i, j int) bool { return projected[i].ID < projected[j].ID })
	fingerprint := sha256.New()
	_ = binary.Write(fingerprint, binary.LittleEndian, ignored[0].ID)
	_ = binary.Write(fingerprint, binary.LittleEndian, uint16(len(ignored[0].Name)))
	_, _ = fingerprint.Write([]byte(ignored[0].Name))
	stats.IgnoredCount = 1
	stats.IgnoredFingerprint = fmt.Sprintf("%x", fingerprint.Sum(nil))
	return projected, stats, nil
}

func compareV2168BiomeMap(label string, retained, comparison map[string]uint32) error {
	if len(comparison) != v2168RetailBiomeCount {
		return fmt.Errorf("%s biome map contains %d records, want %d", label, len(comparison), v2168RetailBiomeCount)
	}
	for name, id := range retained {
		if other, exists := comparison[name]; !exists || other != id {
			return fmt.Errorf("%s biome map disagrees with retained record %q", label, name)
		}
	}
	return nil
}

func encodeV2168BiomeProjection(records []BiomeRecord, stats v2168BiomeProjectionStats) ([]byte, []byte, error) {
	carrier, err := encodeBiomeRegistry(records)
	if err != nil {
		return nil, nil, err
	}
	if len(records) != v2168RetailBiomeCount || stats.IgnoredCount != 1 || len(stats.IgnoredFingerprint) != 64 {
		return nil, nil, errors.New("v2168 biome projection metadata is incomplete")
	}
	carrierSHA := fmt.Sprintf("%x", sha256.Sum256(carrier))
	manifest := v2168BiomeProjectionManifest{
		Schema: "cinnabar.biome-projection.v1", GameVersion: "1.26.40", Protocol: 2168,
		Sources: v2168BiomeProjectionSources{
			BDS: v2168BiomeBDSSource{
				ExecutableSHA256: v2168BDSExecutableSHA256, TableOffset: v2168BiomeTableOffset,
				TableRecords: v2168BiomeSourceCount, TableSHA256: v2168BiomeTableSHA256,
				RetailRecords: v2168RetailBiomeCount, RetailSHA256: v2168RetailTableSHA256,
			},
			PMMP:      v2168BiomePMMPSource{Commit: "bdb44a48fb6beffb6e9f6864f06d2232eb62b6a3", SHA256: v2168PMMPBiomeMapSHA256},
			Dragonfly: v2168BiomeDragonflySource{Module: dragonflyModule, Version: dragonflyVersion, ModuleSum: dragonflyModuleSum},
			Retail: v2168BiomeRetailSource{
				PackageSHA256:   "da77d733fc4bcdf4663933cf066425d1d8388fe649a11e646b1918deacf0a8fe",
				BiomeFileSHA256: "d1b1f59fc8edcd4f970037fe285ccecfcb41ddb878876395c67f24a12e117bbe",
				BDSFileSHA256:   "d1b1f59fc8edcd4f970037fe285ccecfcb41ddb878876395c67f24a12e117bbe",
			},
		},
		Allowlist:  v2168BiomeProjectionAllow{Path: v2168BiomeAllowlistPath, SHA256: v2168BiomeAllowSHA256, Count: v2168RetailBiomeCount},
		Projection: v2168BiomeProjectionSummary{Retained: len(records), IgnoredCount: stats.IgnoredCount, IgnoredFingerprint: stats.IgnoredFingerprint},
		Output:     v2168BiomeProjectionOutput{Format: biomeRegistryHeader, Path: v2168BiomeOutputPath, SHA256: carrierSHA},
	}
	manifestBytes, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil {
		return nil, nil, fmt.Errorf("encode v2168 biome manifest: %w", err)
	}
	return carrier, append(manifestBytes, '\n'), nil
}

func writeV2168BiomeProjection(executablePath, pmmpPath, allowlistPath, outputPath, manifestPath string) error {
	source, err := readV2168BDSBiomeRecords(executablePath)
	if err != nil {
		return err
	}
	allowed, err := readV2168BiomeAllowlist(allowlistPath)
	if err != nil {
		return err
	}
	pmmp, err := readV2168PMMPBiomeMap(pmmpPath)
	if err != nil {
		return err
	}
	dragonfly, err := v2168DragonflyBiomeMap()
	if err != nil {
		return err
	}
	projected, stats, err := projectV2168BiomeRecords(source, allowed, pmmp, dragonfly)
	if err != nil {
		return err
	}
	carrier, manifest, err := encodeV2168BiomeProjection(projected, stats)
	if err != nil {
		return err
	}
	for _, path := range []string{outputPath, manifestPath} {
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			return fmt.Errorf("create v2168 biome output directory: %w", err)
		}
	}
	if err := os.WriteFile(outputPath, carrier, 0o644); err != nil {
		return fmt.Errorf("write v2168 biome registry: %w", err)
	}
	shaPath := strings.TrimSuffix(outputPath, filepath.Ext(outputPath)) + ".sha256"
	if err := os.WriteFile(shaPath, []byte(fmt.Sprintf("%x\n", sha256.Sum256(carrier))), 0o644); err != nil {
		return fmt.Errorf("write v2168 biome checksum: %w", err)
	}
	if err := os.WriteFile(manifestPath, manifest, 0o644); err != nil {
		return fmt.Errorf("write v2168 biome manifest: %w", err)
	}
	return nil
}
