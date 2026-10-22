package proxy

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft/resource"
)

func TestPrependSavedPacks(t *testing.T) {
	tempDir := t.TempDir()
	globalPacksDirOverride = tempDir
	defer func() { globalPacksDirOverride = "" }()

	packUUID := "00112233-4455-6677-8899-aabbccddeeff"
	packBytes := admissionPackArchiveWithID(t, packUUID)
	packPath := filepath.Join(tempDir, packUUID+"-1.0.0-1.mcpack")
	if err := os.WriteFile(packPath, packBytes, 0644); err != nil {
		t.Fatal(err)
	}

	config := globalPacksConfig{
		Active: []struct {
			ID       string `json:"id"`
			SubPack  string `json:"subpack"`
			Revision uint64 `json:"revision"`
		}{
			{
				ID:       packUUID,
				SubPack:  "custom_subpack",
				Revision: 1,
			},
		},
	}
	configData, err := json.Marshal(config)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(tempDir, "global_packs.json"), configData, 0644); err != nil {
		t.Fatal(err)
	}

	serverPackUUID := "11223344-5566-7788-99aa-bbccddeeff00"
	serverPackBytes := admissionPackArchiveWithID(t, serverPackUUID)
	serverPack, err := resource.ReadBytes(serverPackBytes)
	if err != nil {
		t.Fatal(err)
	}

	initialSelected := []sessionPack{
		{
			UUID:    serverPackUUID,
			Version: serverPack.Version(),
			Size:    uint64(len(serverPackBytes)),
		},
	}
	initialPacks := []*resource.Pack{serverPack}

	newSelected, newPacks := prependSavedPacks(initialSelected, initialPacks, nil)
	if len(newSelected) != 2 || len(newPacks) != 2 {
		t.Fatalf("expected 2 packs, got %d selected and %d packs", len(newSelected), len(newPacks))
	}

	if newSelected[0].UUID != packUUID {
		t.Fatalf("expected first selected pack to be %s, got %s", packUUID, newSelected[0].UUID)
	}
	if newSelected[0].SubPack != "custom_subpack" {
		t.Fatalf("expected subpack custom_subpack, got %s", newSelected[0].SubPack)
	}
	if newSelected[1].UUID != serverPackUUID {
		t.Fatalf("expected second selected pack to be %s, got %s", serverPackUUID, newSelected[1].UUID)
	}
}

func TestPrependSavedPacksDeduplication(t *testing.T) {
	tempDir := t.TempDir()
	globalPacksDirOverride = tempDir
	defer func() { globalPacksDirOverride = "" }()

	packUUID := "00112233-4455-6677-8899-aabbccddeeff"
	packBytes := admissionPackArchiveWithID(t, packUUID)
	packPath := filepath.Join(tempDir, packUUID+"-1.0.0-1.mcpack")
	if err := os.WriteFile(packPath, packBytes, 0644); err != nil {
		t.Fatal(err)
	}

	config := globalPacksConfig{
		Active: []struct {
			ID       string `json:"id"`
			SubPack  string `json:"subpack"`
			Revision uint64 `json:"revision"`
		}{
			{
				ID:       packUUID,
				SubPack:  "",
				Revision: 1,
			},
		},
	}
	configData, err := json.Marshal(config)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(tempDir, "global_packs.json"), configData, 0644); err != nil {
		t.Fatal(err)
	}

	// Server also has the same pack UUID
	serverPack, err := resource.ReadBytes(packBytes)
	if err != nil {
		t.Fatal(err)
	}
	initialSelected := []sessionPack{
		{
			UUID:    packUUID,
			Version: serverPack.Version(),
			Size:    uint64(len(packBytes)),
		},
	}
	initialPacks := []*resource.Pack{serverPack}

	newSelected, newPacks := prependSavedPacks(initialSelected, initialPacks, nil)
	// Deduplication should ensure only 1 pack is kept (the saved pack)
	if len(newSelected) != 1 || len(newPacks) != 1 {
		t.Fatalf("expected 1 deduplicated pack, got %d selected and %d packs", len(newSelected), len(newPacks))
	}
	if newSelected[0].UUID != packUUID {
		t.Fatalf("expected pack UUID %s, got %s", packUUID, newSelected[0].UUID)
	}
}
