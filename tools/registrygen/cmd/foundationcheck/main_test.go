package main

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestReadyFoundationFollowsTargetBlockPin(t *testing.T) {
	payload, err := os.ReadFile(filepath.Join("..", "..", "..", "..", "assets", "registry-foundation-v2193.json"))
	if err != nil {
		t.Fatal(err)
	}
	var ready struct {
		ProjectionBindings struct {
			Block struct{ SHA256 string } `json:"block"`
			Light struct{ SHA256 string } `json:"light"`
		} `json:"projection_bindings"`
	}
	if err := json.Unmarshal(payload, &ready); err != nil {
		t.Fatal(err)
	}
	original := ready.ProjectionBindings.Block.SHA256
	changed := strings.Repeat("a", 64)
	root := t.TempDir()
	if err := os.Mkdir(filepath.Join(root, "assets"), 0755); err != nil {
		t.Fatal(err)
	}
	target, err := json.Marshal(map[string]any{"hashes": map[string]string{
		"block_registry": changed, "light_registry": ready.ProjectionBindings.Light.SHA256,
	}})
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "assets", "bedrock-target.json"), target, 0644); err != nil {
		t.Fatal(err)
	}
	t.Chdir(root)
	manifestPath := filepath.Join(root, "foundation.json")
	if err := os.WriteFile(manifestPath, []byte(strings.Replace(string(payload), original, changed, 1)), 0600); err != nil {
		t.Fatal(err)
	}
	if _, _, err := validateFile(manifestPath); err != nil {
		t.Fatalf("rejected current target block binding: %v", err)
	}
	if err := os.WriteFile(manifestPath, payload, 0600); err != nil {
		t.Fatal(err)
	}
	if _, _, err := validateFile(manifestPath); err == nil {
		t.Fatal("accepted stale target block binding")
	}
}
