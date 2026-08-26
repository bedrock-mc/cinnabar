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
		"crates/asset-compiler/src/bin/assetc.rs":   {"vanilla-v2168.mcbea"},
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
	legacyVisualCoverage, err := os.ReadFile(filepath.Join(root, "tools", "visualcoverage", "src", "main.rs"))
	if err != nil {
		t.Fatal(err)
	}
	for _, marker := range []string{"Legacy protocol-1001", "LegacyBaseline1001", "LegacyRatchet1001", "LegacyStrict1001", "LegacyGalleryInventory1001"} {
		if !strings.Contains(string(legacyVisualCoverage), marker) {
			t.Fatalf("historical visual-coverage CLI is not explicitly retired: missing %s", marker)
		}
	}
	for _, activeCommand := range []string{"Command::Baseline", "Command::Ratchet", "Command::Strict", "Command::GalleryInventory"} {
		if strings.Contains(string(legacyVisualCoverage), activeCommand) {
			t.Fatalf("historical visual-coverage CLI still exposes active command %s", activeCommand)
		}
	}
	centralizedAcceptanceConsumers := map[string][]string{
		"scripts/acceptance/Common.ps1":                        {"assets\\bedrock-target.json", "Get-BedrockTargetManifest", "Resolve-BedrockTargetArtifact"},
		"scripts/acceptance/Phase3Launcher.ps1":                {"Get-BedrockTargetManifest", "wire_protocol", "artifacts.physics_registry", "Resolve-BedrockTargetArtifact"},
		"scripts/acceptance/Orchestration/Validate.ps1":        {"Get-BedrockTargetManifest", "Resolve-BedrockTargetArtifact"},
		"scripts/acceptance/Phase3.ps1":                        {"ExpectedProtocol"},
		"scripts/acceptance/FastTransferWitnessValidation.ps1": {"ExpectedProtocol"},
		"scripts/acceptance/FastTransferWitness.ps1":           {"$arguments.Assets"},
		"scripts/acceptance/Galleries/Aquatic.ps1":             {"$registryProtocol = $reader.ReadUInt32()"},
		"scripts/acceptance/Galleries/CrossCrop.ps1":           {"$registryProtocol = $reader.ReadUInt32()"},
		"scripts/acceptance/Galleries/FlowerBed.ps1":           {"$registryProtocol = $reader.ReadUInt32()"},
		"scripts/acceptance/Galleries/SlabStair.ps1":           {"$registryProtocol = $reader.ReadUInt32()"},
		"scripts/acceptance/Galleries/Vine.ps1":                {"$registryProtocol = $reader.ReadUInt32()"},
	}
	for path, required := range centralizedAcceptanceConsumers {
		contents, err := os.ReadFile(filepath.Join(root, filepath.FromSlash(path)))
		if err != nil {
			t.Fatal(err)
		}
		for _, value := range required {
			if !strings.Contains(string(contents), value) {
				t.Fatalf("acceptance consumer %s does not derive %s from the target manifest or registry", path, value)
			}
		}
		if strings.Contains(string(contents), "v1001") || strings.Contains(string(contents), "v2168") {
			t.Fatalf("acceptance consumer %s hard-codes a versioned carrier instead of deriving the target", path)
		}
	}
}
