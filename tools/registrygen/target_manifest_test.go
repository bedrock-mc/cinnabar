package main

import (
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

type bedrockTargetManifest struct {
	Schema       string            `json:"schema"`
	GameVersion  string            `json:"game_version"`
	WireProtocol uint32            `json:"wire_protocol"`
	CodecFeature string            `json:"codec_feature"`
	Hashes       map[string]string `json:"hashes"`
	Artifacts    map[string]string `json:"artifacts"`
}

// TestBedrockTargetManifestOwnsEveryProductionCarrier prevents runtime,
// build, and packaging defaults from selecting protocols independently.
func TestBedrockTargetManifestOwnsEveryProductionCarrier(t *testing.T) {
	root := filepath.Join("..", "..")
	payload, err := os.ReadFile(filepath.Join(root, "assets", "bedrock-target.json"))
	if err != nil {
		t.Fatal(err)
	}
	var target bedrockTargetManifest
	if err := json.Unmarshal(payload, &target); err != nil {
		t.Fatal(err)
	}
	if target.Schema != "cinnabar.bedrock-target.v1" || target.GameVersion != "1.26.40" || target.WireProtocol != 2168 || target.CodecFeature != "bedrock_1_26_44" {
		t.Fatalf("unexpected target identity: %+v", target)
	}
	for name, path := range target.Artifacts {
		if !strings.Contains(path, "2168") || strings.Contains(path, "1001") {
			t.Fatalf("target artifact %s is not protocol-2168-only: %s", name, path)
		}
	}
	for name, expected := range target.Hashes {
		path, ok := target.Artifacts[name]
		if !ok {
			t.Fatalf("target hash %s has no artifact path", name)
		}
		contents, err := os.ReadFile(filepath.Join(root, filepath.FromSlash(path)))
		if err != nil {
			t.Fatal(err)
		}
		if actual := fmt.Sprintf("%x", sha256.Sum256(contents)); actual != expected {
			t.Fatalf("target artifact %s hash %s does not match %s", name, actual, expected)
		}
	}
	consumers := map[string][]string{
		"Makefile": {"assets/bedrock-target.json", target.Artifacts["block_registry"], target.Artifacts["light_registry"], target.Artifacts["biome_registry"], "block-physics-v2168", "vanilla-v2168.mcbea"},
		"app/src/asset_startup/world_provenance.rs": {"block-registry-v2168.bin", "block-light-registry-v2168.bin", "biome-registry-v2168.bin", "bedrock-target.json"},
		"app/src/install_layout.rs":                 {"block-physics-v2168.bin", "vanilla-v2168.mcbea"},
		"tools/dist/src/layout.rs":                  {"block-physics-v2168.bin", "vanilla-v2168.mcbea"},
		"app/src/metrics/diagnostics.rs":            {"block-registry-v2168.bin"},
		"crates/asset-compiler/src/entity/item.rs":  {"block-registry-v2168.bin", "block-item-routes-v2168.json"},
		"crates/protocol/Cargo.toml":                {target.CodecFeature},
	}
	for path, required := range consumers {
		contents, err := os.ReadFile(filepath.Join(root, filepath.FromSlash(path)))
		if err != nil {
			t.Fatal(err)
		}
		for _, value := range required {
			if !strings.Contains(string(contents), value) {
				t.Fatalf("target consumer %s does not derive %s", path, value)
			}
		}
		for _, forbidden := range []string{"block-registry-v1001", "block-light-registry-v1001", "biome-registry-v1001", "block-physics-v1001", "vanilla-v1001.mcbea", "block-item-routes-v1001"} {
			if strings.Contains(string(contents), forbidden) {
				t.Fatalf("production target consumer %s still selects %s", path, forbidden)
			}
		}
	}
}
