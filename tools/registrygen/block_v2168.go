package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/df-mc/dragonfly/server/world"
	"github.com/sandertv/gophertunnel/minecraft/nbt"
	"github.com/segmentio/fasthash/fnv1"
	"github.com/segmentio/fasthash/fnv1a"
)

const (
	v2168BlockProtocol       = 2168
	v2168BlockStateCount     = 17_499
	v2168BlockSourceSHA256   = "1dc6d7ea26b48b5b5e4702762e463b95e59eb109f26c0c3b74115d12cb1941a7"
	v2168BlockSourceSize     = 2_436_125
	v2168RetailItemsSHA256   = "ee8917e7293c89469d6d114cad634eac0b45a702a1d73e2edddd6d5eeee725d0"
	v2168DragonflyVersion    = "v0.11.1"
	v2168DragonflyModuleSum  = "h1:o22uDh6sQsSlrV2ag0BzAT92NDrLR/jZ23kCvMbip6s="
	v2168BlockOutputPath     = "crates/assets/data/block-registry-v2168.bin"
	v2168LightOutputPath     = "crates/assets/data/block-light-registry-v2168.bin"
	v2168BlockManifestSchema = "cinnabar.block-projection.v1"
)

type v2168BlockProjectionManifest struct {
	Schema      string `json:"schema"`
	GameVersion string `json:"game_version"`
	Protocol    uint32 `json:"protocol"`
	Source      struct {
		Module    string `json:"module"`
		Version   string `json:"version"`
		ModuleSum string `json:"module_sum"`
		Commit    string `json:"commit"`
		Blob      string `json:"blob"`
		SHA256    string `json:"sha256"`
		Size      int    `json:"size"`
	} `json:"source"`
	Allowlist struct {
		Path   string `json:"path"`
		SHA256 string `json:"sha256"`
	} `json:"allowlist"`
	Projection struct {
		States            int    `json:"states"`
		LegacyExactStates int    `json:"legacy_exact_states"`
		CurrentAdditions  int    `json:"current_additions"`
		DeniedCount       int    `json:"denied_count"`
		DeniedFingerprint string `json:"denied_fingerprint"`
	} `json:"projection"`
	Output struct {
		Format string `json:"format"`
		Path   string `json:"path"`
		SHA256 string `json:"sha256"`
	} `json:"output"`
	Light *struct {
		Format string `json:"format"`
		Path   string `json:"path"`
		SHA256 string `json:"sha256"`
	} `json:"light,omitempty"`
	LightBlocked *struct {
		UnresolvedCount       int    `json:"unresolved_count"`
		UnresolvedFingerprint string `json:"unresolved_fingerprint"`
	} `json:"light_blocked,omitempty"`
}

type v2168ProjectionStats struct {
	legacy, additions, denied int
	deniedFingerprint         string
}

type v2168SourceEntry struct {
	state   world.BlockState
	ordinal int
}

func orderV2168BlockStates(states []world.BlockState) []v2168SourceEntry {
	ordered := make([]v2168SourceEntry, len(states))
	for index, state := range states {
		ordered[index] = v2168SourceEntry{state: state, ordinal: index}
	}
	sort.SliceStable(ordered, func(i, j int) bool {
		return fnv1.HashString64(ordered[i].state.Name) < fnv1.HashString64(ordered[j].state.Name)
	})
	return ordered
}

func readV2168BlockStates(path string) ([]world.BlockState, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read pinned block states: %w", err)
	}
	if len(data) != v2168BlockSourceSize || fmt.Sprintf("%x", sha256.Sum256(data)) != v2168BlockSourceSHA256 {
		return nil, errors.New("pinned block-state source identity does not match")
	}
	decoder := nbt.NewDecoder(bytes.NewReader(data))
	states := make([]world.BlockState, 0, v2168BlockStateCount)
	for {
		var state world.BlockState
		err := decoder.Decode(&state)
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			if len(states) == v2168BlockStateCount {
				break
			}
			return nil, fmt.Errorf("decode pinned block state %d: %w", len(states), err)
		}
		states = append(states, state)
	}
	if len(states) != v2168BlockStateCount {
		return nil, fmt.Errorf("pinned block-state source contains %d states, want %d", len(states), v2168BlockStateCount)
	}
	return states, nil
}

func v2168SourceRecords(path string) ([]Record, error) {
	states, err := readV2168BlockStates(path)
	if err != nil {
		return nil, err
	}
	// Reproduce Finalize's authoritative stable FNV-1 identifier ordering while
	// preserving source order among states with the same identifier.
	ordered := orderV2168BlockStates(states)
	records := make([]Record, len(ordered))
	seenHashes := make(map[uint32]struct{}, len(ordered))
	for rid, entry := range ordered {
		name, properties := entry.state.Name, entry.state.Properties
		typed, err := typedProperties(properties)
		if err != nil {
			return nil, fmt.Errorf("Dragonfly runtime ID %d: %w", rid, err)
		}
		canonical, err := canonicalTypedState(typed)
		if err != nil {
			return nil, fmt.Errorf("canonicalize Dragonfly runtime ID %d: %w", rid, err)
		}
		networkHash, err := v2168NetworkBlockHash(name, properties)
		if err != nil {
			return nil, fmt.Errorf("network hash for runtime ID %d: %w", rid, err)
		}
		if _, exists := seenHashes[networkHash]; exists {
			return nil, fmt.Errorf("duplicate network hash at runtime ID %d", rid)
		}
		seenHashes[networkHash] = struct{}{}
		records[rid] = Record{SequentialID: uint32(rid), NetworkHash: networkHash, Name: name, StateJSON: canonical, Provenance: ProvenanceDragonfly}
	}
	return records, nil
}

func v2168NetworkBlockHash(name string, properties map[string]any) (uint32, error) {
	if name == "minecraft:unknown" {
		return math.MaxUint32 - 1, nil
	}
	keys := make([]string, 0, len(properties))
	for key := range properties {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	data := []byte{10, 0, 0}
	writeString := func(value string) {
		data = binary.LittleEndian.AppendUint16(data, uint16(len(value)))
		data = append(data, value...)
	}
	data = append(data, 8)
	writeString("name")
	writeString(name)
	data = append(data, 10)
	writeString("states")
	for _, key := range keys {
		switch value := properties[key].(type) {
		case string:
			data = append(data, 8)
			writeString(key)
			writeString(value)
		case uint8:
			data = append(data, 1)
			writeString(key)
			data = append(data, value)
		case int8:
			data = append(data, 1)
			writeString(key)
			data = append(data, byte(value))
		case bool:
			data = append(data, 1)
			writeString(key)
			if value {
				data = append(data, 1)
			} else {
				data = append(data, 0)
			}
		case uint16:
			data = append(data, 2)
			writeString(key)
			data = binary.LittleEndian.AppendUint16(data, value)
		case int16:
			data = append(data, 2)
			writeString(key)
			data = binary.LittleEndian.AppendUint16(data, uint16(value))
		case uint32:
			data = append(data, 3)
			writeString(key)
			data = binary.LittleEndian.AppendUint32(data, value)
		case int32:
			data = append(data, 3)
			writeString(key)
			data = binary.LittleEndian.AppendUint32(data, uint32(value))
		default:
			return 0, fmt.Errorf("unsupported property %q type %T", key, value)
		}
	}
	data = append(data, 0, 0)
	return fnv1a.HashBytes32(data), nil
}

func parseV2168RetailItems(path string) (map[string]struct{}, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read retail item allowlist: %w", err)
	}
	if fmt.Sprintf("%x", sha256.Sum256(data)) != v2168RetailItemsSHA256 {
		return nil, errors.New("retail item allowlist identity does not match")
	}
	allowed := make(map[string]struct{})
	for index, line := range strings.Split(strings.TrimSpace(string(data)), "\n") {
		fields := strings.Split(strings.TrimSuffix(line, "\r"), "\t")
		if len(fields) != 2 || !strings.HasPrefix(fields[1], "minecraft:") {
			return nil, fmt.Errorf("retail item allowlist row %d is malformed", index)
		}
		if _, exists := allowed[fields[1]]; exists {
			return nil, fmt.Errorf("retail item allowlist row %d duplicates an identifier", index)
		}
		allowed[fields[1]] = struct{}{}
	}
	return allowed, nil
}

func projectV2168Blocks(source, legacy []Record, allowed map[string]struct{}) ([]Record, v2168ProjectionStats, map[string]byte, error) {
	if len(source) != v2168BlockStateCount {
		return nil, v2168ProjectionStats{}, nil, errors.New("v2168 block source count does not match")
	}
	legacyByKey := make(map[string]Record, len(legacy))
	for _, record := range legacy {
		if record.Name == retailReservedName {
			continue
		}
		legacyByKey[canonicalRecordKey(record.Name, record.StateJSON)] = record
	}
	projected := make([]Record, len(source))
	classes := make(map[string]byte, len(source))
	deniedHash := sha256.New()
	stats := v2168ProjectionStats{}
	for index, identity := range source {
		key := canonicalRecordKey(identity.Name, identity.StateJSON)
		if old, ok := legacyByKey[key]; ok {
			old.SequentialID, old.NetworkHash = identity.SequentialID, identity.NetworkHash
			old.Name, old.StateJSON = identity.Name, append([]byte(nil), identity.StateJSON...)
			projected[index] = old
			classes[key] = 1
			stats.legacy++
			continue
		}
		if _, ok := allowed[identity.Name]; ok {
			projected[index] = identity
			classes[key] = 2
			stats.additions++
			continue
		}
		projected[index] = Record{
			SequentialID: identity.SequentialID, NetworkHash: identity.NetworkHash,
			Name: retailReservedName, StateJSON: reservedStateJSON(identity.SequentialID),
			ContributorRole: ContributorPrimary, Provenance: ProvenanceDragonfly,
		}
		stats.denied++
		var raw [8]byte
		binary.LittleEndian.PutUint32(raw[:4], identity.SequentialID)
		binary.LittleEndian.PutUint32(raw[4:], identity.NetworkHash)
		deniedHash.Write(raw[:])
		deniedHash.Write([]byte(key))
		deniedHash.Write([]byte{0})
	}
	stats.deniedFingerprint = fmt.Sprintf("%x", deniedHash.Sum(nil))
	return projected, stats, classes, nil
}

func decodeBREGRecords(data []byte, expectedProtocol uint32) (RegistryMetadata, []Record, error) {
	const headerBytes = 8 + 7*4
	const prefixBytes = 24 + 8*4
	if len(data) < headerBytes || string(data[:8]) != registryHeader || binary.LittleEndian.Uint32(data[8:12]) != expectedProtocol {
		return RegistryMetadata{}, nil, fmt.Errorf("input is not protocol-%d BREG1003", expectedProtocol)
	}
	metadata := RegistryMetadata{Protocol: expectedProtocol, CanonicalNames: binary.LittleEndian.Uint32(data[12:16]), CanonicalStates: binary.LittleEndian.Uint32(data[16:20]), ValentineNames: binary.LittleEndian.Uint32(data[20:24]), ValentineStates: binary.LittleEndian.Uint32(data[24:28]), ValentineGapNames: binary.LittleEndian.Uint32(data[28:32]), ValentineGapStates: binary.LittleEndian.Uint32(data[32:36])}
	if metadata.CanonicalStates > maxRecordCount {
		return RegistryMetadata{}, nil, errors.New("BREG record count exceeds limit")
	}
	records := make([]Record, 0, metadata.CanonicalStates)
	cursor := headerBytes
	for index := uint32(0); index < metadata.CanonicalStates; index++ {
		if len(data)-cursor < prefixBytes {
			return RegistryMetadata{}, nil, fmt.Errorf("BREG record %d is truncated", index)
		}
		p := data[cursor : cursor+prefixBytes]
		boxCount := int(p[15])
		nameLen, stateLen := int(binary.LittleEndian.Uint16(p[18:20])), int(binary.LittleEndian.Uint32(p[20:24]))
		if boxCount > maxCollisionBoxesPerRecord || stateLen > maxStateBytes {
			return RegistryMetadata{}, nil, fmt.Errorf("BREG record %d exceeds bounds", index)
		}
		payload := cursor + prefixBytes + boxCount*24
		end := payload + nameLen + stateLen
		if payload < cursor || end < payload || end > len(data) {
			return RegistryMetadata{}, nil, fmt.Errorf("BREG record %d payload is truncated", index)
		}
		record := Record{SequentialID: binary.LittleEndian.Uint32(p[:4]), NetworkHash: binary.LittleEndian.Uint32(p[4:8]), Flags: p[8], ModelFamily: ModelFamily(p[9]), ContributorRole: ContributorRole(p[10]), ModelState: ModelState{Mask: p[11]}, FaceCoverage: p[12], CollisionSeed: CollisionSeed{Confidence: CollisionConfidence(p[13]), ShapeID: binary.LittleEndian.Uint16(p[16:18])}, Provenance: p[14], Name: string(data[payload : payload+nameLen]), StateJSON: append([]byte(nil), data[payload+nameLen:end]...)}
		for field := range record.ModelState.Values {
			record.ModelState.Values[field] = binary.LittleEndian.Uint32(p[24+field*4 : 28+field*4])
		}
		for boxIndex := 0; boxIndex < boxCount; boxIndex++ {
			b := data[cursor+prefixBytes+boxIndex*24 : cursor+prefixBytes+(boxIndex+1)*24]
			record.CollisionSeed.Boxes = append(record.CollisionSeed.Boxes, CollisionBox{MinX: int32(binary.LittleEndian.Uint32(b[0:4])), MinY: int32(binary.LittleEndian.Uint32(b[4:8])), MinZ: int32(binary.LittleEndian.Uint32(b[8:12])), MaxX: int32(binary.LittleEndian.Uint32(b[12:16])), MaxY: int32(binary.LittleEndian.Uint32(b[16:20])), MaxZ: int32(binary.LittleEndian.Uint32(b[20:24]))})
		}
		records = append(records, record)
		cursor = end
	}
	if cursor != len(data) {
		return RegistryMetadata{}, nil, fmt.Errorf("BREG has %d trailing bytes", len(data)-cursor)
	}
	return metadata, records, nil
}

func decodeLREGProperties(data, breg []byte, expectedProtocol uint32, expectedCount int) ([]byte, error) {
	if expectedCount > maxRecordCount || len(data) != 48+expectedCount+sha256.Size || string(data[:8]) != lightRegistryHeader || binary.LittleEndian.Uint32(data[8:12]) != expectedProtocol || int(binary.LittleEndian.Uint32(data[12:16])) != expectedCount {
		return nil, fmt.Errorf("input is not protocol-%d LREG1001 with %d records", expectedProtocol, expectedCount)
	}
	bregDigest := sha256.Sum256(breg)
	if !bytes.Equal(data[16:48], bregDigest[:]) {
		return nil, errors.New("LREG BREG binding mismatch")
	}
	payloadEnd := 48 + expectedCount
	payloadDigest := sha256.Sum256(data[:payloadEnd])
	if !bytes.Equal(data[payloadEnd:], payloadDigest[:]) {
		return nil, errors.New("LREG payload digest mismatch")
	}
	return append([]byte(nil), data[48:payloadEnd]...), nil
}

func resolveV2168Lights(source, projected, legacy []Record, legacyProperties []byte, classes map[string]byte) ([]byte, int, string, error) {
	legacyLights := make(map[string]byte, len(legacy))
	for index, record := range legacy {
		if record.Name != retailReservedName {
			legacyLights[canonicalRecordKey(record.Name, record.StateJSON)] = legacyProperties[index]
		}
	}
	properties := make([]byte, len(projected))
	unresolvedHash := sha256.New()
	unresolved := 0
	for index, record := range projected {
		key := canonicalRecordKey(source[index].Name, source[index].StateJSON)
		switch classes[key] {
		case 0:
			properties[index] = 0
		case 1:
			properties[index] = legacyLights[key]
		case 2:
			// The pinned inputs currently have no retained additions. If that
			// changes, generation fails closed until a hash-bound concrete-accessor
			// table is reviewed for the new exact source.
			unresolved++
			var raw [8]byte
			binary.LittleEndian.PutUint32(raw[:4], record.SequentialID)
			binary.LittleEndian.PutUint32(raw[4:], record.NetworkHash)
			unresolvedHash.Write(raw[:])
			unresolvedHash.Write([]byte(key))
			unresolvedHash.Write([]byte{0})
		}
	}
	fingerprint := fmt.Sprintf("%x", unresolvedHash.Sum(nil))
	return properties, unresolved, fingerprint, nil
}

func encodeResolvedLightRegistryForProtocol(protocol uint32, breg []byte, records []Record, properties []byte) ([]byte, error) {
	if len(records) != len(properties) {
		return nil, errors.New("light property count does not match BREG")
	}
	digest := sha256.Sum256(breg)
	encoded := append([]byte(lightRegistryHeader), make([]byte, 0)...)
	encoded = binary.LittleEndian.AppendUint32(encoded, protocol)
	encoded = binary.LittleEndian.AppendUint32(encoded, uint32(len(records)))
	encoded = append(encoded, digest[:]...)
	encoded = append(encoded, properties...)
	payloadDigest := sha256.Sum256(encoded)
	return append(encoded, payloadDigest[:]...), nil
}

func writeV2168BlockProjection(sourcePath, legacyBREGPath, legacyLightPath, allowlistPath, outputPath, lightOutputPath, manifestPath string) error {
	source, err := v2168SourceRecords(sourcePath)
	if err != nil {
		return err
	}
	legacyBytes, err := os.ReadFile(legacyBREGPath)
	if err != nil {
		return fmt.Errorf("read legacy BREG: %w", err)
	}
	_, legacy, err := decodeBREGRecords(legacyBytes, registryProtocol)
	if err != nil {
		return err
	}
	allowed, err := parseV2168RetailItems(allowlistPath)
	if err != nil {
		return err
	}
	projected, stats, classes, err := projectV2168Blocks(source, legacy, allowed)
	if err != nil {
		return err
	}
	metadata := metadataForRecords(projected)
	metadata.Protocol = v2168BlockProtocol
	encoded, err := encodeWithMetadata(metadata, projected)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(outputPath), 0o755); err != nil {
		return err
	}
	if err := os.WriteFile(outputPath, encoded, 0o644); err != nil {
		return err
	}
	if err := os.WriteFile(outputPath+".sha256", []byte(fmt.Sprintf("%x  %s\n", sha256.Sum256(encoded), filepath.Base(outputPath))), 0o644); err != nil {
		return err
	}

	manifest := v2168BlockProjectionManifest{Schema: v2168BlockManifestSchema, GameVersion: "1.26.40", Protocol: v2168BlockProtocol}
	manifest.Source.Module, manifest.Source.Version, manifest.Source.ModuleSum = dragonflyModule, v2168DragonflyVersion, v2168DragonflyModuleSum
	manifest.Source.Commit, manifest.Source.Blob, manifest.Source.SHA256, manifest.Source.Size = "0c2c404540fc651873c24a020b0a48778bd56295", "7006d9d46217425aab8e7d998f70c370b6b9c4eb", v2168BlockSourceSHA256, v2168BlockSourceSize
	manifest.Allowlist.Path, manifest.Allowlist.SHA256 = "crates/protocol/data/retail_items_1_26_40.tsv", v2168RetailItemsSHA256
	manifest.Projection.States, manifest.Projection.LegacyExactStates, manifest.Projection.CurrentAdditions, manifest.Projection.DeniedCount, manifest.Projection.DeniedFingerprint = len(projected), stats.legacy, stats.additions, stats.denied, stats.deniedFingerprint
	manifest.Output.Format, manifest.Output.Path, manifest.Output.SHA256 = registryHeader, v2168BlockOutputPath, fmt.Sprintf("%x", sha256.Sum256(encoded))

	legacyLightBytes, err := os.ReadFile(legacyLightPath)
	if err != nil {
		return fmt.Errorf("read legacy LREG: %w", err)
	}
	legacyProperties, err := decodeLREGProperties(legacyLightBytes, legacyBytes, registryProtocol, len(legacy))
	if err != nil {
		return err
	}
	properties, unresolved, unresolvedFingerprint, err := resolveV2168Lights(source, projected, legacy, legacyProperties, classes)
	if err != nil {
		return err
	}
	if unresolved == 0 {
		light, err := encodeResolvedLightRegistryForProtocol(v2168BlockProtocol, encoded, projected, properties)
		if err != nil {
			return err
		}
		if lightOutputPath == "" {
			return errors.New("resolved v2168 lights require -light-out")
		}
		if err := os.WriteFile(lightOutputPath, light, 0o644); err != nil {
			return err
		}
		if err := os.WriteFile(lightOutputPath+".sha256", []byte(fmt.Sprintf("%x  %s\n", sha256.Sum256(light), filepath.Base(lightOutputPath))), 0o644); err != nil {
			return err
		}
		manifest.Light = &struct {
			Format string `json:"format"`
			Path   string `json:"path"`
			SHA256 string `json:"sha256"`
		}{lightRegistryHeader, v2168LightOutputPath, fmt.Sprintf("%x", sha256.Sum256(light))}
	} else {
		manifest.LightBlocked = &struct {
			UnresolvedCount       int    `json:"unresolved_count"`
			UnresolvedFingerprint string `json:"unresolved_fingerprint"`
		}{unresolved, unresolvedFingerprint}
	}
	manifestBytes, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil {
		return err
	}
	manifestBytes = append(manifestBytes, '\n')
	if err := os.WriteFile(manifestPath, manifestBytes, 0o644); err != nil {
		return err
	}
	if unresolved != 0 {
		return fmt.Errorf("v2168 light projection has %d unresolved retained states", unresolved)
	}
	return nil
}
