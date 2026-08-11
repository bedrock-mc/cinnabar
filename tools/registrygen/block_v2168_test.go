package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/df-mc/dragonfly/server/world"
	"github.com/segmentio/fasthash/fnv1"
)

func TestV2168CheckedArtifactsAreExactBoundAndLegacyIsByteIdentical(t *testing.T) {
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

	breg, err := os.ReadFile(filepath.Join(root, "crates", "assets", "data", "block-registry-v2168.bin"))
	if err != nil {
		t.Fatal(err)
	}
	metadata, records, err := decodeBREGRecords(breg, v2168BlockProtocol)
	if err != nil {
		t.Fatal(err)
	}
	if metadata.CanonicalStates != v2168BlockStateCount || len(records) != v2168BlockStateCount {
		t.Fatalf("v2168 states = %d", len(records))
	}
	for index, record := range records {
		if record.SequentialID != uint32(index) {
			t.Fatalf("runtime ID %d is %d", index, record.SequentialID)
		}
	}
	if got := strings.ToLower(hexDigest(breg)); got != "e3768f6d70195b22ac3843f6ef49261a80cd83284bc9741c7eb4a446def6bec8" {
		t.Fatalf("BREG SHA-256 = %s", got)
	}
	lreg, err := os.ReadFile(filepath.Join(root, "crates", "assets", "data", "block-light-registry-v2168.bin"))
	if err != nil {
		t.Fatal(err)
	}
	properties, err := decodeLREGProperties(lreg, breg, v2168BlockProtocol, len(records))
	if err != nil {
		t.Fatal(err)
	}
	if len(properties) != len(records) || hexDigest(lreg) != "88bac8fd074e392930321d12f46b291f0557d89dd87392a13fb3b5025bfcd272" {
		t.Fatal("v2168 LREG identity mismatch")
	}
	transplanted := 0
	for index, record := range records {
		if record.Name == retailReservedName {
			if properties[index] != 0 {
				t.Fatalf("reserved runtime ID %d has light %#x", index, properties[index])
			}
			continue
		}
		want, ok := legacyLights[canonicalRecordKey(record.Name, record.StateJSON)]
		if !ok || properties[index] != want {
			t.Fatalf("runtime ID %d is not an exact legacy-key light transplant", index)
		}
		transplanted++
	}
	if transplanted != 16_530 {
		t.Fatalf("transplanted light states = %d", transplanted)
	}
}

func TestV2168NetworkHashUsesCanonicalLittleEndianNBT(t *testing.T) {
	propertiesA := map[string]any{"z": int32(0x01020304), "a": uint8(1)}
	propertiesB := map[string]any{"a": uint8(1), "z": int32(0x01020304)}
	first, err := v2168NetworkBlockHash("minecraft:test", propertiesA)
	if err != nil {
		t.Fatal(err)
	}
	second, err := v2168NetworkBlockHash("minecraft:test", propertiesB)
	if err != nil {
		t.Fatal(err)
	}
	if first != second || first != 0x15556e09 {
		t.Fatalf("canonical network hash = %#x/%#x", first, second)
	}
}

func TestV2168OrderingIsStableFNV1ByIdentifier(t *testing.T) {
	states := []world.BlockState{
		{Name: "minecraft:z", Version: 1},
		{Name: "minecraft:a", Properties: map[string]any{"order": int32(1)}, Version: 1},
		{Name: "minecraft:a", Properties: map[string]any{"order": int32(2)}, Version: 1},
	}
	ordered := orderV2168BlockStates(states)
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

func TestV2168ProjectionIsDeterministicAndDefaultDeniesWithoutChangingIdentity(t *testing.T) {
	legacy := []Record{{SequentialID: 4, NetworkHash: 40, Name: "minecraft:known", StateJSON: []byte(`{}`), Flags: flagCubeGeometry | flagOccludesFullFace, Provenance: ProvenanceDragonfly}}
	source := []Record{
		{SequentialID: 0, NetworkHash: 10, Name: "minecraft:known", StateJSON: []byte(`{}`), Provenance: ProvenanceDragonfly},
		{SequentialID: 1, NetworkHash: 11, Name: "minecraft:unlisted", StateJSON: []byte(`{}`), Provenance: ProvenanceDragonfly},
	}
	first, firstStats, _, err := projectV2168BlocksForTest(source, legacy, map[string]struct{}{})
	if err != nil {
		t.Fatal(err)
	}
	second, secondStats, _, err := projectV2168BlocksForTest(source, legacy, map[string]struct{}{})
	if err != nil {
		t.Fatal(err)
	}
	if !recordsEqual(first[0], second[0]) || !recordsEqual(first[1], second[1]) || firstStats != secondStats {
		t.Fatal("projection is not deterministic")
	}
	denied := first[1]
	if denied.SequentialID != 1 || denied.NetworkHash != 11 || denied.Name != retailReservedName || !bytes.Equal(denied.StateJSON, reservedStateJSON(1)) {
		t.Fatalf("denied identity was not neutralized safely: %+v", denied)
	}
}

func projectV2168BlocksForTest(source, legacy []Record, allowed map[string]struct{}) ([]Record, v2168ProjectionStats, map[string]byte, error) {
	padded := make([]Record, v2168BlockStateCount)
	copy(padded, source)
	for index := len(source); index < len(padded); index++ {
		padded[index] = Record{SequentialID: uint32(index), NetworkHash: uint32(index + 100), Name: "minecraft:padding", StateJSON: []byte(`{"i":{"type":"int","value":0}}`), Provenance: ProvenanceDragonfly}
	}
	return projectV2168Blocks(padded, legacy, allowed)
}

func TestV2168ManifestPublishesOnlyDeniedAggregate(t *testing.T) {
	payload, err := os.ReadFile(filepath.Join("..", "..", "assets", "block-projection-v2168.json"))
	if err != nil {
		t.Fatal(err)
	}
	var manifest v2168BlockProjectionManifest
	decoder := json.NewDecoder(bytes.NewReader(payload))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&manifest); err != nil {
		t.Fatal(err)
	}
	if manifest.Projection.DeniedCount != 969 || len(manifest.Projection.DeniedFingerprint) != 64 || manifest.Projection.CurrentAdditions != 0 {
		t.Fatalf("projection aggregate = %+v", manifest.Projection)
	}
	if manifest.Source.Version != v2168DragonflyVersion || manifest.Source.ModuleSum != v2168DragonflyModuleSum ||
		manifest.Source.SHA256 != v2168BlockSourceSHA256 || manifest.Source.Size != v2168BlockSourceSize {
		t.Fatalf("source identity = %+v", manifest.Source)
	}
	if bytes.Contains(bytes.ToLower(payload), []byte("denied_names")) || bytes.Contains(bytes.ToLower(payload), []byte("unresolved_names")) {
		t.Fatal("manifest exposes excluded identifiers")
	}
}

func TestV2168DecodersRejectWrongProtocolCrossHashTrailingAndMalformed(t *testing.T) {
	root := filepath.Join("..", "..", "crates", "assets", "data")
	breg, _ := os.ReadFile(filepath.Join(root, "block-registry-v2168.bin"))
	lreg, _ := os.ReadFile(filepath.Join(root, "block-light-registry-v2168.bin"))
	if _, _, err := decodeBREGRecords(breg, registryProtocol); err == nil {
		t.Fatal("accepted wrong BREG protocol")
	}
	if _, _, err := decodeBREGRecords(append(append([]byte(nil), breg...), 0), v2168BlockProtocol); err == nil {
		t.Fatal("accepted trailing BREG")
	}
	wrong := append([]byte(nil), lreg...)
	wrong[16] ^= 1
	if _, err := decodeLREGProperties(wrong, breg, v2168BlockProtocol, v2168BlockStateCount); err == nil {
		t.Fatal("accepted cross-hash LREG")
	}
	malformed := append([]byte(nil), lreg...)
	malformed[48] ^= 1
	if _, err := decodeLREGProperties(malformed, breg, v2168BlockProtocol, v2168BlockStateCount); err == nil {
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
