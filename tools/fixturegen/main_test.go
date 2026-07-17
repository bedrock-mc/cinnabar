package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft/protocol"
)

type testManifestEntry struct {
	Name          string `json:"name"`
	File          string `json:"file"`
	ID            uint32 `json:"id"`
	ByteLength    int    `json:"byte_length"`
	SHA256        string `json:"sha256"`
	WireAuthority string `json:"wire_authority,omitempty"`
	WireCommit    string `json:"wire_commit,omitempty"`
}

func TestGenerateIsDeterministicAndWritesPinnedRawBatches(t *testing.T) {
	firstDir := t.TempDir()
	secondDir := t.TempDir()
	if err := generate(firstDir); err != nil {
		t.Fatalf("generate first corpus: %v", err)
	}
	if err := generate(secondDir); err != nil {
		t.Fatalf("generate second corpus: %v", err)
	}

	firstManifestBytes := readFile(t, filepath.Join(firstDir, "manifest.json"))
	secondManifestBytes := readFile(t, filepath.Join(secondDir, "manifest.json"))
	if !bytes.Equal(firstManifestBytes, secondManifestBytes) {
		t.Fatal("manifest differs between identical generator runs")
	}
	if len(firstManifestBytes) == 0 || firstManifestBytes[len(firstManifestBytes)-1] != '\n' {
		t.Fatal("manifest must end in exactly one newline")
	}

	var manifest []testManifestEntry
	if err := json.Unmarshal(firstManifestBytes, &manifest); err != nil {
		t.Fatalf("decode manifest: %v", err)
	}
	wantNames := []string{
		"NetworkSettings",
		"StartGame",
		"LevelChunk",
		"MovePlayer",
		"PlayerAuthInput",
		"AddActor",
		"Text",
		"SetTitle",
		"BossEvent",
		"ModalFormRequest",
		"AvailableCommands",
		"AvailableCommandsLive356513",
		"CraftingDataMaterialReducer",
		"BiomeDefinitionListChunkGeneration",
	}
	wantIDs := []uint32{143, 11, 58, 19, 144, 13, 9, 88, 74, 100, 76, 76, 52, 122}
	wantHeaders := [][]byte{
		{0x8f, 0x49},
		{0x8b, 0x48},
		{0xba, 0x48},
		{0x93, 0x48},
		{0x90, 0x49},
		{0x8d, 0x48},
		{0x89, 0x48},
		{0xd8, 0x48},
		{0xca, 0x48},
		{0xe4, 0x48},
		{0xcc, 0x48},
		{0xcc, 0x48},
		{0xb4, 0x48},
		{0xfa, 0x48},
	}
	if len(manifest) != len(wantNames) {
		t.Fatalf("manifest entries = %d, want %d", len(manifest), len(wantNames))
	}

	for i, entry := range manifest {
		if entry.Name != wantNames[i] || entry.ID != wantIDs[i] {
			t.Fatalf("entry %d identity = (%q, %d), want (%q, %d)", i, entry.Name, entry.ID, wantNames[i], wantIDs[i])
		}
		first := readFile(t, filepath.Join(firstDir, entry.File))
		second := readFile(t, filepath.Join(secondDir, entry.File))
		if !bytes.Equal(first, second) {
			t.Fatalf("%s differs between identical generator runs", entry.Name)
		}
		if len(first) != entry.ByteLength {
			t.Fatalf("%s byte length = %d, manifest says %d", entry.Name, len(first), entry.ByteLength)
		}
		digest := sha256.Sum256(first)
		if got := hex.EncodeToString(digest[:]); got != entry.SHA256 {
			t.Fatalf("%s sha256 = %s, manifest says %s", entry.Name, got, entry.SHA256)
		}
		if len(first) < 2 || first[0] != 0xfe {
			t.Fatalf("%s does not begin with raw batch header 0xfe", entry.Name)
		}

		payload := bytes.NewBuffer(first[1:])
		var declared uint32
		if err := protocol.Varuint32(payload, &declared); err != nil {
			t.Fatalf("%s length varuint: %v", entry.Name, err)
		}
		if int(declared) != payload.Len() {
			t.Fatalf("%s declared entry length = %d, remaining = %d", entry.Name, declared, payload.Len())
		}
		if got := payload.Bytes()[:2]; !reflect.DeepEqual(got, wantHeaders[i]) {
			t.Fatalf("%s header bytes = %x, want %x", entry.Name, got, wantHeaders[i])
		}
		if entry.Name == "AvailableCommandsLive356513" {
			const packetHeaderBytes = 2
			if got := payload.Len() - packetHeaderBytes; got != 356_513 {
				t.Fatalf("live AvailableCommands body length = %d, want 356513", got)
			}
		}
		if entry.Name == "BiomeDefinitionListChunkGeneration" {
			if entry.ByteLength != 48 {
				t.Fatalf("biome definition fixture length = %d, want 48", entry.ByteLength)
			}
			if entry.SHA256 != "a1a626d9b27cd943bc38fbbc356a09ea711ddb26acad72e284dd8dfaff94fbd4" {
				t.Fatalf("biome definition fixture sha256 = %s", entry.SHA256)
			}
			if entry.WireAuthority != "hashimthearab/gophertunnel" || entry.WireCommit != "9948b1729395d2e819fce28e079d4a7bfc67716c" {
				t.Fatalf("biome definition fixture provenance = (%q, %q)", entry.WireAuthority, entry.WireCommit)
			}
		}
	}
}

func readFile(t *testing.T, path string) []byte {
	t.Helper()
	b, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read %s: %v", path, err)
	}
	return b
}
