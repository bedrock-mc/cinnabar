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
	"slices"
	"sort"
	"strings"

	shared "github.com/bedrock-mc/protocolgen/generated/data"
	"github.com/bedrock-mc/protocolgen/generated/data/registry"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/sandertv/gophertunnel/minecraft/nbt"
	"github.com/segmentio/fasthash/fnv1"
	"github.com/segmentio/fasthash/fnv1a"
)

const (
	v2193BlockProtocol       = 2193
	v2193GameVersion         = "1.26.50"
	v2193BlockStateCount     = 22_091
	v2193RetailItemsPath     = "crates/protocol/data/retail_items_1_26_50.tsv"
	v2193RetailItemsSHA256   = "6f186e8f781c611722cd28ece47f643112732a89e18cd9beab9d414243750821"
	v2193BlockOutputPath     = "crates/assets/data/block-registry-v2193.bin"
	v2193LightOutputPath     = "crates/assets/data/block-light-registry-v2193.bin"
	v2193BlockManifestSchema = "cinnabar.block-projection.v2"
)

type v2193BlockProjectionManifest struct {
	Schema      string `json:"schema"`
	GameVersion string `json:"game_version"`
	Protocol    uint32 `json:"protocol"`
	Source      struct {
		Module           string `json:"module"`
		SourceLockSHA256 string `json:"source_lock_sha256"`
		CloudburstRef    string `json:"cloudburst_ref"`
	} `json:"source"`
	Allowlist struct {
		Path   string `json:"path"`
		SHA256 string `json:"sha256"`
	} `json:"allowlist"`
	Projection struct {
		States              int      `json:"states"`
		LegacyExactStates   int      `json:"legacy_exact_states"`
		LegacyReducedStates int      `json:"legacy_reduced_states"`
		TwinStates          int      `json:"twin_states"`
		TwinNames           int      `json:"twin_names"`
		TwinFingerprint     string   `json:"twin_fingerprint"`
		UnresolvedRetail    []string `json:"unresolved_retail"`
		DeniedCount         int      `json:"denied_count"`
		DeniedFingerprint   string   `json:"denied_fingerprint"`
		EducationStates     int      `json:"education_states"`
	} `json:"projection"`
	Output struct {
		Format string `json:"format"`
		Path   string `json:"path"`
		SHA256 string `json:"sha256"`
	} `json:"output"`
	Light struct {
		Format string `json:"format"`
		Path   string `json:"path"`
		SHA256 string `json:"sha256"`
	} `json:"light"`
}

type v2193ProjectionStats struct {
	legacy, reduced, twins, additions, denied, education int
	deniedFingerprint, additionFingerprint               string
}

type v2193SourceEntry struct {
	state   world.BlockState
	ordinal int
}

func orderV2193BlockStates(states []world.BlockState) []v2193SourceEntry {
	ordered := make([]v2193SourceEntry, len(states))
	for index, state := range states {
		ordered[index] = v2193SourceEntry{state: state, ordinal: index}
	}
	sort.SliceStable(ordered, func(i, j int) bool {
		return fnv1.HashString64(ordered[i].state.Name) < fnv1.HashString64(ordered[j].state.Name)
	})
	return ordered
}

// readV2193BlockStates reads the palette from the shared authenticated catalog.
func readV2193BlockStates() ([]world.BlockState, error) {
	if err := validateSharedTarget(); err != nil {
		return nil, err
	}
	return decodeSharedBlockStates(registry.BlockStatesNBT())
}

// decodeSharedBlockStates rejects truncated compounds and unexpected palette sizes.
func decodeSharedBlockStates(data []byte) ([]world.BlockState, error) {
	reader := bytes.NewReader(data)
	decoder := nbt.NewDecoder(reader)
	states := make([]world.BlockState, 0, v2193BlockStateCount)
	for reader.Len() != 0 {
		var state world.BlockState
		err := decoder.Decode(&state)
		if err != nil {
			return nil, fmt.Errorf("decode pinned block state %d: %w", len(states), err)
		}
		states = append(states, state)
	}
	if len(states) != v2193BlockStateCount {
		return nil, fmt.Errorf("pinned block-state source contains %d states, want %d", len(states), v2193BlockStateCount)
	}
	return states, nil
}

// v2193SourceRecords converts shared palette states to the client carrier identity format.
func v2193SourceRecords() ([]Record, error) {
	states, err := readV2193BlockStates()
	if err != nil {
		return nil, err
	}
	// Reproduce Finalize's authoritative stable FNV-1 identifier ordering while
	// preserving source order among states with the same identifier.
	ordered := orderV2193BlockStates(states)
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
		networkHash, err := v2193NetworkBlockHash(name, properties)
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

func v2193NetworkBlockHash(name string, properties map[string]any) (uint32, error) {
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

func parseV2193RetailItems(path string) (map[string]struct{}, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read retail item allowlist: %w", err)
	}
	if fmt.Sprintf("%x", sha256.Sum256(data)) != v2193RetailItemsSHA256 {
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

// Projection classes recorded per canonical source key.
const (
	v2193ClassDenied byte = iota
	v2193ClassLegacyExact
	v2193ClassAddition
	v2193ClassLegacyReduced
	v2193ClassTwin
	v2193ClassEducation
)

// v2193ConnectionProperties are the 1.26.50 neighbour-derived state properties.
// The client derives the same shapes from neighbours, so a state that differs
// from a protocol-1001 state only by these keeps that state's reviewed facts.
func v2193IsConnectionProperty(name string) bool {
	return strings.HasPrefix(name, "minecraft:connection_") || name == "minecraft:corner"
}

func v2193ReducedState(state []byte) ([]byte, error) {
	var properties map[string]json.RawMessage
	if err := json.Unmarshal(state, &properties); err != nil {
		return nil, fmt.Errorf("decode canonical state: %w", err)
	}
	for name := range properties {
		if v2193IsConnectionProperty(name) {
			delete(properties, name)
		}
	}
	return json.Marshal(properties)
}

// v2193Projection records, per source index, the projected record, its class,
// and the protocol-1001 record index that supplied its facts (-1 if none).
type v2193Projection struct {
	records []Record
	classes []byte
	facts   []int
}

func projectV2193Blocks(source, legacy []Record, allowed map[string]struct{}) (v2193Projection, v2193ProjectionStats, error) {
	if len(source) != v2193BlockStateCount {
		return v2193Projection{}, v2193ProjectionStats{}, errors.New("v2193 block source count does not match")
	}
	legacyByKey := make(map[string]int, len(legacy))
	for index, record := range legacy {
		if record.Name == retailReservedName {
			continue
		}
		legacyByKey[canonicalRecordKey(record.Name, record.StateJSON)] = index
	}
	projection := v2193Projection{
		records: make([]Record, len(source)), classes: make([]byte, len(source)),
		facts: make([]int, len(source)),
	}
	projected := projection.records
	deniedHash := sha256.New()
	additionHash := sha256.New()
	stats := v2193ProjectionStats{}
	adopt := func(index int, identity Record, legacyIndex int, class byte) {
		facts := legacy[legacyIndex]
		facts.SequentialID, facts.NetworkHash = identity.SequentialID, identity.NetworkHash
		facts.Name, facts.StateJSON = identity.Name, append([]byte(nil), identity.StateJSON...)
		facts.CollisionSeed.Boxes = append([]CollisionBox(nil), facts.CollisionSeed.Boxes...)
		projected[index] = facts
		projection.classes[index], projection.facts[index] = class, legacyIndex
	}
	for index, identity := range source {
		projection.facts[index] = -1
		if record, matched, err := educationRenderRecord(identity); err != nil {
			return v2193Projection{}, v2193ProjectionStats{}, err
		} else if matched {
			projected[index] = record
			projection.classes[index] = v2193ClassEducation
			stats.education++
			continue
		}
		key := canonicalRecordKey(identity.Name, identity.StateJSON)
		if old, ok := legacyByKey[key]; ok {
			adopt(index, identity, old, v2193ClassLegacyExact)
			stats.legacy++
			continue
		}
		reduced, err := v2193ReducedState(identity.StateJSON)
		if err != nil {
			return v2193Projection{}, v2193ProjectionStats{}, fmt.Errorf("runtime ID %d: %w", identity.SequentialID, err)
		}
		if old, ok := legacyByKey[canonicalRecordKey(identity.Name, reduced)]; ok {
			adopt(index, identity, old, v2193ClassLegacyReduced)
			stats.reduced++
			continue
		}
		twin, twinned := v2193TwinFor(identity.Name)
		item := identity.Name
		if twinned {
			item = twin.item
		}
		_, retail := allowed[item]
		if !retail || slices.Contains(v2193UnresolvedRetail, identity.Name) {
			projected[index] = Record{
				SequentialID: identity.SequentialID, NetworkHash: identity.NetworkHash,
				Name: retailReservedName, StateJSON: reservedStateJSON(identity.SequentialID),
				ContributorRole: ContributorPrimary, Provenance: ProvenanceDragonfly,
			}
			stats.denied++
			writeV2193Fingerprint(deniedHash, identity, key)
			continue
		}
		if twinned {
			old, found := legacyByKey[canonicalRecordKey(twin.twin, reduced)]
			if !found {
				return v2193Projection{}, v2193ProjectionStats{}, fmt.Errorf("reviewed twin %s has no protocol-1001 state for %s %s", twin.twin, identity.Name, reduced)
			}
			adopt(index, identity, old, v2193ClassTwin)
			stats.twins++
			continue
		}
		projected[index] = identity
		projection.classes[index] = v2193ClassAddition
		stats.additions++
		writeV2193Fingerprint(additionHash, identity, key)
	}
	for index := range projected {
		if err := applyFamilyRouting(&projected[index]); err != nil {
			return v2193Projection{}, v2193ProjectionStats{}, err
		}
	}
	stats.deniedFingerprint = fmt.Sprintf("%x", deniedHash.Sum(nil))
	stats.additionFingerprint = fmt.Sprintf("%x", additionHash.Sum(nil))
	return projection, stats, nil
}

func writeV2193Fingerprint(hash io.Writer, identity Record, key string) {
	var raw [8]byte
	binary.LittleEndian.PutUint32(raw[:4], identity.SequentialID)
	binary.LittleEndian.PutUint32(raw[4:], identity.NetworkHash)
	hash.Write(raw[:])
	hash.Write([]byte(key))
	hash.Write([]byte{0})
}

func decodeBREGRecords(data []byte, expectedProtocol uint32) (RegistryMetadata, []Record, error) {
	const headerBytes = registryHeaderBytes
	const prefixBytes = recordHeaderBytes
	if len(data) < headerBytes || string(data[:8]) != registryHeader || binary.LittleEndian.Uint32(data[8:12]) != expectedProtocol {
		return RegistryMetadata{}, nil, fmt.Errorf("input is not protocol-%d BREG1003", expectedProtocol)
	}
	metadata := decodeRegistryMetadata(data[len(registryHeader):headerBytes])
	if metadata.CanonicalStates > maxRecordCount {
		return RegistryMetadata{}, nil, errors.New("BREG record count exceeds limit")
	}
	records := make([]Record, 0, metadata.CanonicalStates)
	cursor := headerBytes
	for index := uint32(0); index < metadata.CanonicalStates; index++ {
		if len(data)-cursor < prefixBytes {
			return RegistryMetadata{}, nil, fmt.Errorf("BREG record %d is truncated", index)
		}
		p := decodeRecordHeader(data[cursor : cursor+prefixBytes])
		boxCount := int(p.BoxCount)
		nameLen, stateLen := int(p.NameLen), int(p.StateLen)
		if boxCount > maxCollisionBoxesPerRecord || stateLen > maxStateBytes {
			return RegistryMetadata{}, nil, fmt.Errorf("BREG record %d exceeds bounds", index)
		}
		payload := cursor + prefixBytes + boxCount*collisionBoxBytes
		end := payload + nameLen + stateLen
		if payload < cursor || end < payload || end > len(data) {
			return RegistryMetadata{}, nil, fmt.Errorf("BREG record %d payload is truncated", index)
		}
		record := Record{
			SequentialID: p.SequentialID, NetworkHash: p.NetworkHash, Flags: p.RawFlags,
			ModelFamily: ModelFamily(p.ModelFamily), ContributorRole: ContributorRole(p.ContributorRole),
			ModelState: ModelState{Mask: p.ModelMask, Values: p.Values}, FaceCoverage: p.FaceCoverage,
			CollisionSeed: CollisionSeed{Confidence: CollisionConfidence(p.Confidence), ShapeID: p.ShapeID},
			Provenance:    p.RawProvenance, Name: string(data[payload : payload+nameLen]),
			StateJSON: append([]byte(nil), data[payload+nameLen:end]...),
		}
		for boxIndex := 0; boxIndex < boxCount; boxIndex++ {
			start := cursor + prefixBytes + boxIndex*collisionBoxBytes
			record.CollisionSeed.Boxes = append(record.CollisionSeed.Boxes, decodeCollisionBox(data[start:start+collisionBoxBytes]))
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

// writeV2193BlockProjection combines shared facts with the client's reviewed rendering and retail admission policy.
func writeV2193BlockProjection(legacyBREGPath, allowlistPath, outputPath, lightOutputPath, manifestPath string) error {
	if lightOutputPath == "" {
		return errors.New("v2193 block projection requires -light-out")
	}
	source, err := v2193SourceRecords()
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
	allowed, err := parseV2193RetailItems(allowlistPath)
	if err != nil {
		return err
	}
	projection, stats, err := projectV2193Blocks(source, legacy, allowed)
	if err != nil {
		return err
	}
	if stats.additions != 0 {
		return fmt.Errorf("v2193 projection has %d retail states without reviewed facts (fingerprint %s)", stats.additions, stats.additionFingerprint)
	}
	if stats.education != v2193EducationStateCount {
		return fmt.Errorf("Education construction palette contains %d states, want %d", stats.education, v2193EducationStateCount)
	}
	projected := projection.records
	properties, err := applySharedBlockFacts(projected)
	if err != nil {
		return err
	}
	metadata := metadataForRecords(projected)
	metadata.Protocol = v2193BlockProtocol
	encoded, err := encodeWithMetadata(metadata, projected)
	if err != nil {
		return err
	}

	light, err := encodeResolvedLightRegistryForProtocol(v2193BlockProtocol, encoded, projected, properties)
	if err != nil {
		return err
	}

	twinNames := make([]string, 0, len(v2193Twins))
	for name := range v2193Twins {
		twinNames = append(twinNames, name)
	}
	sort.Strings(twinNames)
	twinHash := sha256.New()
	for _, name := range twinNames {
		fmt.Fprintf(twinHash, "%s\x00%s\x00%s\x00", name, v2193Twins[name].twin, v2193Twins[name].item)
	}
	manifest := v2193BlockProjectionManifest{Schema: v2193BlockManifestSchema, GameVersion: v2193GameVersion, Protocol: v2193BlockProtocol}
	manifest.Source.Module = "github.com/bedrock-mc/protocolgen/generated/data"
	manifest.Source.SourceLockSHA256, manifest.Source.CloudburstRef = shared.SourceLockSHA256, shared.CloudburstRef
	manifest.Allowlist.Path, manifest.Allowlist.SHA256 = v2193RetailItemsPath, v2193RetailItemsSHA256
	manifest.Projection.States, manifest.Projection.LegacyExactStates = len(projected), stats.legacy
	manifest.Projection.LegacyReducedStates, manifest.Projection.TwinStates = stats.reduced, stats.twins
	manifest.Projection.TwinNames, manifest.Projection.TwinFingerprint = len(twinNames), fmt.Sprintf("%x", twinHash.Sum(nil))
	manifest.Projection.UnresolvedRetail = append([]string(nil), v2193UnresolvedRetail...)
	manifest.Projection.DeniedCount, manifest.Projection.DeniedFingerprint = stats.denied, stats.deniedFingerprint
	manifest.Projection.EducationStates = stats.education
	manifest.Output.Format, manifest.Output.Path, manifest.Output.SHA256 = registryHeader, v2193BlockOutputPath, fmt.Sprintf("%x", sha256.Sum256(encoded))
	manifest.Light.Format, manifest.Light.Path, manifest.Light.SHA256 = lightRegistryHeader, v2193LightOutputPath, fmt.Sprintf("%x", sha256.Sum256(light))
	manifestBytes, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil {
		return err
	}
	manifestBytes = append(manifestBytes, '\n')

	for _, path := range []string{outputPath, lightOutputPath, manifestPath} {
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			return err
		}
	}
	for _, output := range []struct {
		path string
		data []byte
	}{{outputPath, encoded}, {lightOutputPath, light}} {
		if err := os.WriteFile(output.path, output.data, 0o644); err != nil {
			return err
		}
		checksum := fmt.Sprintf("%x  %s\n", sha256.Sum256(output.data), filepath.Base(output.path))
		if err := os.WriteFile(output.path+".sha256", []byte(checksum), 0o644); err != nil {
			return err
		}
	}
	return os.WriteFile(manifestPath, manifestBytes, 0o644)
}

// v2193Twin names the protocol-1001 block whose identical state schema and
// shape supply facts for a retail 1.26.50 block, and the retail item that
// admits it.
type v2193Twin struct{ twin, item string }

// v2193Twins covers every retail 1.26.50 block without a protocol-1001 state.
// Model, collision, light and friction follow the twin; the identity and the
// resource-pack visual name stay the new block's.
var v2193Twins = map[string]v2193Twin{
	"minecraft:black_concrete_double_slab":      {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:black_concrete_slab"},
	"minecraft:black_concrete_slab":             {twin: "minecraft:smooth_stone_slab", item: "minecraft:black_concrete_slab"},
	"minecraft:black_concrete_stairs":           {twin: "minecraft:stone_stairs", item: "minecraft:black_concrete_stairs"},
	"minecraft:black_wool_double_slab":          {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:black_wool_slab"},
	"minecraft:black_wool_slab":                 {twin: "minecraft:smooth_stone_slab", item: "minecraft:black_wool_slab"},
	"minecraft:black_wool_stairs":               {twin: "minecraft:stone_stairs", item: "minecraft:black_wool_stairs"},
	"minecraft:blue_concrete_double_slab":       {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:blue_concrete_slab"},
	"minecraft:blue_concrete_slab":              {twin: "minecraft:smooth_stone_slab", item: "minecraft:blue_concrete_slab"},
	"minecraft:blue_concrete_stairs":            {twin: "minecraft:stone_stairs", item: "minecraft:blue_concrete_stairs"},
	"minecraft:blue_wool_double_slab":           {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:blue_wool_slab"},
	"minecraft:blue_wool_slab":                  {twin: "minecraft:smooth_stone_slab", item: "minecraft:blue_wool_slab"},
	"minecraft:blue_wool_stairs":                {twin: "minecraft:stone_stairs", item: "minecraft:blue_wool_stairs"},
	"minecraft:brown_concrete_double_slab":      {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:brown_concrete_slab"},
	"minecraft:brown_concrete_slab":             {twin: "minecraft:smooth_stone_slab", item: "minecraft:brown_concrete_slab"},
	"minecraft:brown_concrete_stairs":           {twin: "minecraft:stone_stairs", item: "minecraft:brown_concrete_stairs"},
	"minecraft:brown_wool_double_slab":          {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:brown_wool_slab"},
	"minecraft:brown_wool_slab":                 {twin: "minecraft:smooth_stone_slab", item: "minecraft:brown_wool_slab"},
	"minecraft:brown_wool_stairs":               {twin: "minecraft:stone_stairs", item: "minecraft:brown_wool_stairs"},
	"minecraft:cyan_concrete_double_slab":       {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:cyan_concrete_slab"},
	"minecraft:cyan_concrete_slab":              {twin: "minecraft:smooth_stone_slab", item: "minecraft:cyan_concrete_slab"},
	"minecraft:cyan_concrete_stairs":            {twin: "minecraft:stone_stairs", item: "minecraft:cyan_concrete_stairs"},
	"minecraft:cyan_wool_double_slab":           {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:cyan_wool_slab"},
	"minecraft:cyan_wool_slab":                  {twin: "minecraft:smooth_stone_slab", item: "minecraft:cyan_wool_slab"},
	"minecraft:cyan_wool_stairs":                {twin: "minecraft:stone_stairs", item: "minecraft:cyan_wool_stairs"},
	"minecraft:gray_concrete_double_slab":       {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:gray_concrete_slab"},
	"minecraft:gray_concrete_slab":              {twin: "minecraft:smooth_stone_slab", item: "minecraft:gray_concrete_slab"},
	"minecraft:gray_concrete_stairs":            {twin: "minecraft:stone_stairs", item: "minecraft:gray_concrete_stairs"},
	"minecraft:gray_wool_double_slab":           {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:gray_wool_slab"},
	"minecraft:gray_wool_slab":                  {twin: "minecraft:smooth_stone_slab", item: "minecraft:gray_wool_slab"},
	"minecraft:gray_wool_stairs":                {twin: "minecraft:stone_stairs", item: "minecraft:gray_wool_stairs"},
	"minecraft:green_concrete_double_slab":      {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:green_concrete_slab"},
	"minecraft:green_concrete_slab":             {twin: "minecraft:smooth_stone_slab", item: "minecraft:green_concrete_slab"},
	"minecraft:green_concrete_stairs":           {twin: "minecraft:stone_stairs", item: "minecraft:green_concrete_stairs"},
	"minecraft:green_wool_double_slab":          {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:green_wool_slab"},
	"minecraft:green_wool_slab":                 {twin: "minecraft:smooth_stone_slab", item: "minecraft:green_wool_slab"},
	"minecraft:green_wool_stairs":               {twin: "minecraft:stone_stairs", item: "minecraft:green_wool_stairs"},
	"minecraft:light_blue_concrete_double_slab": {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:light_blue_concrete_slab"},
	"minecraft:light_blue_concrete_slab":        {twin: "minecraft:smooth_stone_slab", item: "minecraft:light_blue_concrete_slab"},
	"minecraft:light_blue_concrete_stairs":      {twin: "minecraft:stone_stairs", item: "minecraft:light_blue_concrete_stairs"},
	"minecraft:light_blue_wool_double_slab":     {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:light_blue_wool_slab"},
	"minecraft:light_blue_wool_slab":            {twin: "minecraft:smooth_stone_slab", item: "minecraft:light_blue_wool_slab"},
	"minecraft:light_blue_wool_stairs":          {twin: "minecraft:stone_stairs", item: "minecraft:light_blue_wool_stairs"},
	"minecraft:light_gray_concrete_double_slab": {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:light_gray_concrete_slab"},
	"minecraft:light_gray_concrete_slab":        {twin: "minecraft:smooth_stone_slab", item: "minecraft:light_gray_concrete_slab"},
	"minecraft:light_gray_concrete_stairs":      {twin: "minecraft:stone_stairs", item: "minecraft:light_gray_concrete_stairs"},
	"minecraft:light_gray_wool_double_slab":     {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:light_gray_wool_slab"},
	"minecraft:light_gray_wool_slab":            {twin: "minecraft:smooth_stone_slab", item: "minecraft:light_gray_wool_slab"},
	"minecraft:light_gray_wool_stairs":          {twin: "minecraft:stone_stairs", item: "minecraft:light_gray_wool_stairs"},
	"minecraft:lime_concrete_double_slab":       {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:lime_concrete_slab"},
	"minecraft:lime_concrete_slab":              {twin: "minecraft:smooth_stone_slab", item: "minecraft:lime_concrete_slab"},
	"minecraft:lime_concrete_stairs":            {twin: "minecraft:stone_stairs", item: "minecraft:lime_concrete_stairs"},
	"minecraft:lime_wool_double_slab":           {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:lime_wool_slab"},
	"minecraft:lime_wool_slab":                  {twin: "minecraft:smooth_stone_slab", item: "minecraft:lime_wool_slab"},
	"minecraft:lime_wool_stairs":                {twin: "minecraft:stone_stairs", item: "minecraft:lime_wool_stairs"},
	"minecraft:magenta_concrete_double_slab":    {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:magenta_concrete_slab"},
	"minecraft:magenta_concrete_slab":           {twin: "minecraft:smooth_stone_slab", item: "minecraft:magenta_concrete_slab"},
	"minecraft:magenta_concrete_stairs":         {twin: "minecraft:stone_stairs", item: "minecraft:magenta_concrete_stairs"},
	"minecraft:magenta_wool_double_slab":        {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:magenta_wool_slab"},
	"minecraft:magenta_wool_slab":               {twin: "minecraft:smooth_stone_slab", item: "minecraft:magenta_wool_slab"},
	"minecraft:magenta_wool_stairs":             {twin: "minecraft:stone_stairs", item: "minecraft:magenta_wool_stairs"},
	"minecraft:orange_concrete_double_slab":     {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:orange_concrete_slab"},
	"minecraft:orange_concrete_slab":            {twin: "minecraft:smooth_stone_slab", item: "minecraft:orange_concrete_slab"},
	"minecraft:orange_concrete_stairs":          {twin: "minecraft:stone_stairs", item: "minecraft:orange_concrete_stairs"},
	"minecraft:orange_poplar_leaves":            {twin: "minecraft:oak_leaves", item: "minecraft:orange_poplar_leaves"},
	"minecraft:orange_wool_double_slab":         {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:orange_wool_slab"},
	"minecraft:orange_wool_slab":                {twin: "minecraft:smooth_stone_slab", item: "minecraft:orange_wool_slab"},
	"minecraft:orange_wool_stairs":              {twin: "minecraft:stone_stairs", item: "minecraft:orange_wool_stairs"},
	"minecraft:pink_concrete_double_slab":       {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:pink_concrete_slab"},
	"minecraft:pink_concrete_slab":              {twin: "minecraft:smooth_stone_slab", item: "minecraft:pink_concrete_slab"},
	"minecraft:pink_concrete_stairs":            {twin: "minecraft:stone_stairs", item: "minecraft:pink_concrete_stairs"},
	"minecraft:pink_wool_double_slab":           {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:pink_wool_slab"},
	"minecraft:pink_wool_slab":                  {twin: "minecraft:smooth_stone_slab", item: "minecraft:pink_wool_slab"},
	"minecraft:pink_wool_stairs":                {twin: "minecraft:stone_stairs", item: "minecraft:pink_wool_stairs"},
	"minecraft:poplar_button":                   {twin: "minecraft:wooden_button", item: "minecraft:poplar_button"},
	"minecraft:poplar_door":                     {twin: "minecraft:wooden_door", item: "minecraft:poplar_door"},
	"minecraft:poplar_double_slab":              {twin: "minecraft:oak_double_slab", item: "minecraft:poplar_slab"},
	"minecraft:poplar_fence":                    {twin: "minecraft:oak_fence", item: "minecraft:poplar_fence"},
	"minecraft:poplar_fence_gate":               {twin: "minecraft:fence_gate", item: "minecraft:poplar_fence_gate"},
	"minecraft:poplar_hanging_sign":             {twin: "minecraft:oak_hanging_sign", item: "minecraft:poplar_hanging_sign"},
	"minecraft:poplar_log":                      {twin: "minecraft:oak_log", item: "minecraft:poplar_log"},
	"minecraft:poplar_planks":                   {twin: "minecraft:oak_planks", item: "minecraft:poplar_planks"},
	"minecraft:poplar_pressure_plate":           {twin: "minecraft:wooden_pressure_plate", item: "minecraft:poplar_pressure_plate"},
	"minecraft:poplar_sapling":                  {twin: "minecraft:oak_sapling", item: "minecraft:poplar_sapling"},
	"minecraft:poplar_shelf":                    {twin: "minecraft:oak_shelf", item: "minecraft:poplar_shelf"},
	"minecraft:poplar_slab":                     {twin: "minecraft:oak_slab", item: "minecraft:poplar_slab"},
	"minecraft:poplar_stairs":                   {twin: "minecraft:oak_stairs", item: "minecraft:poplar_stairs"},
	"minecraft:poplar_standing_sign":            {twin: "minecraft:standing_sign", item: "minecraft:poplar_sign"},
	"minecraft:poplar_trapdoor":                 {twin: "minecraft:trapdoor", item: "minecraft:poplar_trapdoor"},
	"minecraft:poplar_wall_sign":                {twin: "minecraft:wall_sign", item: "minecraft:poplar_sign"},
	"minecraft:poplar_wood":                     {twin: "minecraft:oak_wood", item: "minecraft:poplar_wood"},
	"minecraft:purple_concrete_double_slab":     {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:purple_concrete_slab"},
	"minecraft:purple_concrete_slab":            {twin: "minecraft:smooth_stone_slab", item: "minecraft:purple_concrete_slab"},
	"minecraft:purple_concrete_stairs":          {twin: "minecraft:stone_stairs", item: "minecraft:purple_concrete_stairs"},
	"minecraft:purple_wool_double_slab":         {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:purple_wool_slab"},
	"minecraft:purple_wool_slab":                {twin: "minecraft:smooth_stone_slab", item: "minecraft:purple_wool_slab"},
	"minecraft:purple_wool_stairs":              {twin: "minecraft:stone_stairs", item: "minecraft:purple_wool_stairs"},
	"minecraft:red_concrete_double_slab":        {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:red_concrete_slab"},
	"minecraft:red_concrete_slab":               {twin: "minecraft:smooth_stone_slab", item: "minecraft:red_concrete_slab"},
	"minecraft:red_concrete_stairs":             {twin: "minecraft:stone_stairs", item: "minecraft:red_concrete_stairs"},
	"minecraft:red_poplar_leaves":               {twin: "minecraft:oak_leaves", item: "minecraft:red_poplar_leaves"},
	"minecraft:red_shrub":                       {twin: "minecraft:deadbush", item: "minecraft:red_shrub"},
	"minecraft:red_wool_double_slab":            {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:red_wool_slab"},
	"minecraft:red_wool_slab":                   {twin: "minecraft:smooth_stone_slab", item: "minecraft:red_wool_slab"},
	"minecraft:red_wool_stairs":                 {twin: "minecraft:stone_stairs", item: "minecraft:red_wool_stairs"},
	"minecraft:stripped_poplar_log":             {twin: "minecraft:stripped_oak_log", item: "minecraft:stripped_poplar_log"},
	"minecraft:stripped_poplar_wood":            {twin: "minecraft:stripped_oak_wood", item: "minecraft:stripped_poplar_wood"},
	"minecraft:white_concrete_double_slab":      {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:white_concrete_slab"},
	"minecraft:white_concrete_slab":             {twin: "minecraft:smooth_stone_slab", item: "minecraft:white_concrete_slab"},
	"minecraft:white_concrete_stairs":           {twin: "minecraft:stone_stairs", item: "minecraft:white_concrete_stairs"},
	"minecraft:white_wool_double_slab":          {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:white_wool_slab"},
	"minecraft:white_wool_slab":                 {twin: "minecraft:smooth_stone_slab", item: "minecraft:white_wool_slab"},
	"minecraft:white_wool_stairs":               {twin: "minecraft:stone_stairs", item: "minecraft:white_wool_stairs"},
	"minecraft:yellow_concrete_double_slab":     {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:yellow_concrete_slab"},
	"minecraft:yellow_concrete_slab":            {twin: "minecraft:smooth_stone_slab", item: "minecraft:yellow_concrete_slab"},
	"minecraft:yellow_concrete_stairs":          {twin: "minecraft:stone_stairs", item: "minecraft:yellow_concrete_stairs"},
	"minecraft:yellow_poplar_leaves":            {twin: "minecraft:oak_leaves", item: "minecraft:yellow_poplar_leaves"},
	"minecraft:yellow_wool_double_slab":         {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:yellow_wool_slab"},
	"minecraft:yellow_wool_slab":                {twin: "minecraft:smooth_stone_slab", item: "minecraft:yellow_wool_slab"},
	"minecraft:yellow_wool_stairs":              {twin: "minecraft:stone_stairs", item: "minecraft:yellow_wool_stairs"},
}

// v2193UnresolvedRetail are retail 1.26.50 blocks with no schema-identical
// twin; they stay reserved until a reviewed fact source exists.
var v2193UnresolvedRetail = []string{"minecraft:shelf_mushroom", "minecraft:straw_bed"}

func v2193TwinFor(name string) (v2193Twin, bool) {
	twin, ok := v2193Twins[name]
	return twin, ok
}
