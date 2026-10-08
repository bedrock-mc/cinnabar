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
	var ready manifest
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
	ready.ProjectionBindings.Block.SHA256 = changed
	if err := validate(ready); err != nil {
		t.Fatalf("rejected current target block binding: %v", err)
	}
	ready.ProjectionBindings.Block.SHA256 = original
	if err := validate(ready); err == nil {
		t.Fatal("accepted stale target block binding")
	}
}
