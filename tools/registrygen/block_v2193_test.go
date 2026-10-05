package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/json"
	"github.com/hashimthearab/rust-mcbe/tools/registrygen/internal/targetpin"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/df-mc/dragonfly/server/world"
	"github.com/segmentio/fasthash/fnv1"
)

func TestV2193CheckedArtifactsAreExactBoundAndLegacyIsByteIdentical(t *testing.T) {
	root := filepath.Join("..", "..")
	legacyBytes, err := os.ReadFile(filepath.Join(root, "crates", "assets", "data", "block-registry-v1001.bin"))
	if err != nil {
		t.Fatal(err)
	}
	legacyMetadata, legacy, err := decodeBREGRecords(legacyBytes, registryProtocol)
	if err != nil {
		t.Fatal(err)
	}
	reencodedLegacy, err := encodeWithMetadata(legacyMetadata, legacy)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(reencodedLegacy, legacyBytes) {
		t.Fatal("legacy BREG changed during decode/encode identity round trip")
	}
	legacyLightBytes, err := os.ReadFile(filepath.Join(root, "crates", "assets", "data", "block-light-registry-v1001.bin"))
	if err != nil {
		t.Fatal(err)
	}
	legacyProperties, err := decodeLREGProperties(legacyLightBytes, legacyBytes, registryProtocol, len(legacy))
	if err != nil {
		t.Fatal(err)
	}
	legacyLights := make(map[string]byte, len(legacy))
	for index, record := range legacy {
		if record.Name != retailReservedName {
			legacyLights[canonicalRecordKey(record.Name, record.StateJSON)] = legacyProperties[index]
		}
	}
	factKey := func(record Record) string {
		reduced, err := v2193ReducedState(record.StateJSON)
		if err != nil {
			t.Fatal(err)
		}
		if _, ok := legacyLights[canonicalRecordKey(record.Name, record.StateJSON)]; ok {
			return canonicalRecordKey(record.Name, record.StateJSON)
		}
		if twin, ok := v2193TwinFor(record.Name); ok {
			if _, exact := legacyLights[canonicalRecordKey(record.Name, reduced)]; !exact {
				return canonicalRecordKey(twin.twin, reduced)
			}
		}
		return canonicalRecordKey(record.Name, reduced)
	}

	breg, err := os.ReadFile(filepath.Join(root, "crates", "assets", "data", "block-registry-v2193.bin"))
	if err != nil {
		t.Fatal(err)
	}
	metadata, records, err := decodeBREGRecords(breg, v2193BlockProtocol)
	if err != nil {
		t.Fatal(err)
	}
	if metadata.CanonicalStates != v2193BlockStateCount || len(records) != v2193BlockStateCount {
		t.Fatalf("v2193 states = %d", len(records))
	}
	for index, record := range records {
		if record.SequentialID != uint32(index) {
			t.Fatalf("runtime ID %d is %d", index, record.SequentialID)
		}
	}
	if got := strings.ToLower(hexDigest(breg)); got != "04984b63037cda766e9a41b81bb1314e0c649b6f999bb27d56730decb3c7be53" {
		t.Fatalf("BREG SHA-256 = %s", got)
	}
	lreg, err := os.ReadFile(filepath.Join(root, "crates", "assets", "data", "block-light-registry-v2193.bin"))
	if err != nil {
		t.Fatal(err)
	}
	properties, err := decodeLREGProperties(lreg, breg, v2193BlockProtocol, len(records))
	if err != nil {
		t.Fatal(err)
	}
	lightHash, err := targetpin.LightHash()
	if err != nil {
		t.Fatal(err)
	}
	if len(properties) != len(records) || hexDigest(lreg) != lightHash {
		t.Fatal("v2193 LREG identity mismatch")
	}
	transplanted := 0
	for index, record := range records {
		if record.Name == retailReservedName {
			if properties[index] != 0 {
				t.Fatalf("reserved runtime ID %d has light %#x", index, properties[index])
			}
			continue
		}
		want, ok := legacyLights[factKey(record)]
		// Final native registrations override the legacy filter for these
		// types. Keep the independent emission nibble from the fact source.
		switch record.Name {
		case "minecraft:portal":
			// The unknown-axis legacy row has no emitter implementation.
			// Native registration sets both properties for all portal states.
			want = 11
		case "minecraft:snow_layer":
			want &= 0x0f
		case "minecraft:water":
			want = want&0x0f | 1<<4
		case "minecraft:flowing_water":
			want = want&0x0f | 2<<4
		case "minecraft:ice", "minecraft:frosted_ice":
			want = want&0x0f | 3<<4
		}
		// A legacy unimplemented-block default (emission 0, filter 15) may be corrected.
		defaulted := want == unknownBlockEmission|unknownBlockFilter<<4
		if !ok || (properties[index] != want && !defaulted) {
			t.Fatalf("runtime ID %d (%s) light = %#x, want %#x after native overrides",
				index, record.Name, properties[index], want)
		}
		transplanted++
	}
	if transplanted != 15_963+3_440+2_026 {
		t.Fatalf("transplanted light states = %d", transplanted)
	}
}

func TestV2193NetworkHashUsesCanonicalLittleEndianNBT(t *testing.T) {
	propertiesA := map[string]any{"z": int32(0x01020304), "a": uint8(1)}
	propertiesB := map[string]any{"a": uint8(1), "z": int32(0x01020304)}
	first, err := v2193NetworkBlockHash("minecraft:test", propertiesA)
	if err != nil {
		t.Fatal(err)
	}
	second, err := v2193NetworkBlockHash("minecraft:test", propertiesB)
	if err != nil {
		t.Fatal(err)
	}
	if first != second || first != 0x15556e09 {
		t.Fatalf("canonical network hash = %#x/%#x", first, second)
	}
}

func TestV2193OrderingIsStableFNV1ByIdentifier(t *testing.T) {
	states := []world.BlockState{
		{Name: "minecraft:z", Version: 1},
		{Name: "minecraft:a", Properties: map[string]any{"order": int32(1)}, Version: 1},
		{Name: "minecraft:a", Properties: map[string]any{"order": int32(2)}, Version: 1},
	}
	ordered := orderV2193BlockStates(states)
	var equalOrdinals []int
	for _, entry := range ordered {
		if entry.state.Name == "minecraft:a" {
			equalOrdinals = append(equalOrdinals, entry.ordinal)
		}
	}
	if len(equalOrdinals) != 2 || equalOrdinals[0] != 1 || equalOrdinals[1] != 2 {
		t.Fatalf("equal-identifier order = %v", equalOrdinals)
	}
	for index := 1; index < len(ordered); index++ {
		left := fnv1.HashString64(ordered[index-1].state.Name)
		right := fnv1.HashString64(ordered[index].state.Name)
		if left > right {
			t.Fatalf("FNV-1 order decreases at %d", index)
		}
	}
}

func TestV2193ProjectionIsDeterministicAndDefaultDeniesWithoutChangingIdentity(t *testing.T) {
	legacy := []Record{{SequentialID: 4, NetworkHash: 40, Name: "minecraft:known", StateJSON: []byte(`{}`), Flags: flagCubeGeometry | flagOccludesFullFace, Provenance: ProvenanceDragonfly}}
	source := []Record{
		{SequentialID: 0, NetworkHash: 10, Name: "minecraft:known", StateJSON: []byte(`{}`), Provenance: ProvenanceDragonfly},
		{SequentialID: 1, NetworkHash: 11, Name: "minecraft:unlisted", StateJSON: []byte(`{}`), Provenance: ProvenanceDragonfly},
	}
	first, firstStats, err := projectV2193BlocksForTest(source, legacy, map[string]struct{}{})
	if err != nil {
		t.Fatal(err)
	}
	second, secondStats, err := projectV2193BlocksForTest(source, legacy, map[string]struct{}{})
	if err != nil {
		t.Fatal(err)
	}
	if !recordsEqual(first.records[0], second.records[0]) || !recordsEqual(first.records[1], second.records[1]) || firstStats != secondStats {
		t.Fatal("projection is not deterministic")
	}
	denied := first.records[1]
	if denied.SequentialID != 1 || denied.NetworkHash != 11 || denied.Name != retailReservedName || !bytes.Equal(denied.StateJSON, reservedStateJSON(1)) {
		t.Fatalf("denied identity was not neutralized safely: %+v", denied)
	}
}

// Connection-only state keys keep the unconnected legacy facts under the new identity.
func TestV2193ConnectionStatesReduceToLegacyFacts(t *testing.T) {
	legacy := []Record{{SequentialID: 7, NetworkHash: 70, Name: "minecraft:oak_fence", StateJSON: []byte(`{}`), ModelFamily: ModelFamilyFence, Provenance: ProvenanceDragonfly}}
	connected := []byte(`{"minecraft:connection_east":{"type":"byte","value":1},"minecraft:connection_north":{"type":"byte","value":0},"minecraft:connection_south":{"type":"byte","value":0},"minecraft:connection_west":{"type":"byte","value":1}}`)
	source := []Record{{SequentialID: 0, NetworkHash: 12, Name: "minecraft:oak_fence", StateJSON: connected, Provenance: ProvenanceDragonfly}}
	projection, stats, err := projectV2193BlocksForTest(source, legacy, map[string]struct{}{})
	if err != nil {
		t.Fatal(err)
	}
	got := projection.records[0]
	if stats.reduced != 1 || projection.classes[0] != v2193ClassLegacyReduced || projection.facts[0] != 0 {
		t.Fatalf("connected fence was not reduced: %+v", stats)
	}
	if got.ModelFamily != ModelFamilyFence || got.NetworkHash != 12 || got.SequentialID != 0 || !bytes.Equal(got.StateJSON, connected) {
		t.Fatalf("reduced record = %+v", got)
	}
}

// A twinned block needs its admitting item and borrows the twin's same-state facts.
func TestV2193TwinsRequireRetailItemAndSchemaIdenticalTwinState(t *testing.T) {
	half := []byte(`{"minecraft:vertical_half":{"type":"string","value":"top"}}`)
	legacy := []Record{{SequentialID: 3, NetworkHash: 30, Name: "minecraft:smooth_stone_double_slab", StateJSON: half, ModelFamily: ModelFamilyCube, Provenance: ProvenanceDragonfly}}
	source := []Record{{SequentialID: 0, NetworkHash: 13, Name: "minecraft:white_wool_double_slab", StateJSON: half, Provenance: ProvenanceDragonfly}}
	denied, stats, err := projectV2193BlocksForTest(source, legacy, map[string]struct{}{})
	if err != nil || stats.twins != 0 || denied.records[0].Name != retailReservedName {
		t.Fatalf("twin admitted without its item: %+v %v", stats, err)
	}
	twinned, stats, err := projectV2193BlocksForTest(source, legacy, map[string]struct{}{"minecraft:white_wool_slab": {}})
	if err != nil {
		t.Fatal(err)
	}
	got := twinned.records[0]
	if stats.twins != 1 || twinned.twins[0] != "minecraft:smooth_stone_double_slab" || got.Name != "minecraft:white_wool_double_slab" || got.ModelFamily != ModelFamilyCube || got.NetworkHash != 13 {
		t.Fatalf("twin record = %+v %+v", got, stats)
	}
	if _, _, err := projectV2193BlocksForTest(source, nil, map[string]struct{}{"minecraft:white_wool_slab": {}}); err == nil {
		t.Fatal("accepted a twin without its legacy state")
	}
}

func projectV2193BlocksForTest(source, legacy []Record, allowed map[string]struct{}) (v2193Projection, v2193ProjectionStats, error) {
	padded := make([]Record, v2193BlockStateCount)
	copy(padded, source)
	for index := len(source); index < len(padded); index++ {
		padded[index] = Record{SequentialID: uint32(index), NetworkHash: uint32(index + 100), Name: "minecraft:padding", StateJSON: []byte(`{"i":{"type":"int","value":0}}`), Provenance: ProvenanceDragonfly}
	}
	return projectV2193Blocks(padded, legacy, allowed)
}

func TestV2193ManifestPublishesOnlyDeniedAggregate(t *testing.T) {
	payload, err := os.ReadFile(filepath.Join("..", "..", "assets", "block-projection-v2193.json"))
	if err != nil {
		t.Fatal(err)
	}
	var manifest v2193BlockProjectionManifest
	decoder := json.NewDecoder(bytes.NewReader(payload))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&manifest); err != nil {
		t.Fatal(err)
	}
	projection := manifest.Projection
	if projection.DeniedCount != 662 || len(projection.DeniedFingerprint) != 64 || projection.TwinNames != len(v2193Twins) ||
		projection.States != projection.LegacyExactStates+projection.LegacyReducedStates+projection.TwinStates+projection.DeniedCount {
		t.Fatalf("projection aggregate = %+v", projection)
	}
	if manifest.Source.Version != v2193DragonflyVersion || manifest.Source.ModuleSum != v2193DragonflyModuleSum ||
		manifest.Source.SHA256 != v2193BlockSourceSHA256 || manifest.Source.Size != v2193BlockSourceSize {
		t.Fatalf("source identity = %+v", manifest.Source)
	}
	if bytes.Contains(bytes.ToLower(payload), []byte("denied_names")) || bytes.Contains(bytes.ToLower(payload), []byte("unresolved_names")) {
		t.Fatal("manifest exposes excluded identifiers")
	}
}

func TestV2193DecodersRejectWrongProtocolCrossHashTrailingAndMalformed(t *testing.T) {
	root := filepath.Join("..", "..", "crates", "assets", "data")
	breg, _ := os.ReadFile(filepath.Join(root, "block-registry-v2193.bin"))
	lreg, _ := os.ReadFile(filepath.Join(root, "block-light-registry-v2193.bin"))
	if _, _, err := decodeBREGRecords(breg, registryProtocol); err == nil {
		t.Fatal("accepted wrong BREG protocol")
	}
	if _, _, err := decodeBREGRecords(append(append([]byte(nil), breg...), 0), v2193BlockProtocol); err == nil {
		t.Fatal("accepted trailing BREG")
	}
	wrong := append([]byte(nil), lreg...)
	wrong[16] ^= 1
	if _, err := decodeLREGProperties(wrong, breg, v2193BlockProtocol, v2193BlockStateCount); err == nil {
		t.Fatal("accepted cross-hash LREG")
	}
	malformed := append([]byte(nil), lreg...)
	malformed[48] ^= 1
	if _, err := decodeLREGProperties(malformed, breg, v2193BlockProtocol, v2193BlockStateCount); err == nil {
		t.Fatal("accepted malformed LREG")
	}
}

func hexDigest(data []byte) string { return strings.ToLower(fmtSHA(sha256.Sum256(data))) }
func fmtSHA(digest [32]byte) string {
	const digits = "0123456789abcdef"
	out := make([]byte, 64)
	for i, value := range digest {
		out[i*2], out[i*2+1] = digits[value>>4], digits[value&15]
	}
	return string(out)
}
