package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
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

func effectiveTrackedAttributes(root, carrier string) (map[string]string, error) {
	command := exec.Command("git", "-C", root, "check-attr", "--cached", "-z", "text", "eol", "--", carrier)
	output, err := command.CombinedOutput()
	if err != nil {
		return nil, fmt.Errorf("git check-attr failed: %w: %s", err, output)
	}
	fields := bytes.Split(output, []byte{0})
	if len(fields) == 0 || len(fields[len(fields)-1]) != 0 {
		return nil, fmt.Errorf("git check-attr returned malformed NUL-delimited output")
	}
	fields = fields[:len(fields)-1]
	if len(fields)%3 != 0 {
		return nil, fmt.Errorf("git check-attr returned %d fields, want path/attribute/value triples", len(fields))
	}
	attributes := make(map[string]string, len(fields)/3)
	for index := 0; index < len(fields); index += 3 {
		if checkedPath := string(fields[index]); checkedPath != carrier {
			return nil, fmt.Errorf("git check-attr reported path %q, want %q", checkedPath, carrier)
		}
		attributes[string(fields[index+1])] = string(fields[index+2])
	}
	return attributes, nil
}

func TestEffectiveTrackedAttributesHonorLaterOverride(t *testing.T) {
	root := t.TempDir()
	if output, err := exec.Command("git", "init", "--quiet", root).CombinedOutput(); err != nil {
		t.Fatalf("git init failed: %v: %s", err, output)
	}
	attributesPath := filepath.Join(root, ".gitattributes")
	if err := os.WriteFile(attributesPath, []byte("*.json text eol=lf\ncarrier.json text eol=crlf\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if output, err := exec.Command("git", "-C", root, "add", ".gitattributes").CombinedOutput(); err != nil {
		t.Fatalf("git add failed: %v: %s", err, output)
	}
	attributes, err := effectiveTrackedAttributes(root, "carrier.json")
	if err != nil {
		t.Fatal(err)
	}
	if attributes["text"] != "set" || attributes["eol"] != "crlf" {
		t.Fatalf("later override did not control effective attributes: %+v", attributes)
	}
}

func TestBedrockTargetCarrierAutocrlfUpgrade(t *testing.T) {
	for _, lineEnding := range []struct{ name, value string }{{"LF", "\n"}, {"CRLF", "\r\n"}} {
		t.Run(lineEnding.name, func(t *testing.T) {
			testBedrockTargetCarrierAutocrlfUpgrade(t, lineEnding.value)
		})
	}
}

func testBedrockTargetCarrierAutocrlfUpgrade(t *testing.T, attributeLineEnding string) {
	t.Helper()
	sourceRoot, err := filepath.Abs(filepath.Join("..", ".."))
	if err != nil {
		t.Fatal(err)
	}
	carrierPath := "crates/assets/data/block-item-routes-v2168.json"
	manifestPath := "assets/bedrock-target.json"
	attributePath := ".gitattributes"
	readSource := func(path string) []byte {
		t.Helper()
		contents, err := os.ReadFile(filepath.Join(sourceRoot, filepath.FromSlash(path)))
		if err != nil {
			t.Fatal(err)
		}
		return contents
	}
	candidateCarrier := readSource(carrierPath)
	candidateManifest := readSource(manifestPath)
	candidateAttributes := bytes.ReplaceAll(readSource(attributePath), []byte("\r\n"), []byte("\n"))
	candidateAttributes = bytes.ReplaceAll(candidateAttributes, []byte("\n"), []byte(attributeLineEnding))
	baseCarrier := bytes.Replace(candidateCarrier, []byte(`"schema" :`), []byte(`"schema":`), 1)
	if bytes.Equal(baseCarrier, candidateCarrier) {
		t.Fatal("active carrier lacks the semantic-neutral upgrade byte revision")
	}
	var baseSemantic, candidateSemantic bytes.Buffer
	if err := json.Compact(&baseSemantic, baseCarrier); err != nil {
		t.Fatal(err)
	}
	if err := json.Compact(&candidateSemantic, candidateCarrier); err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(baseSemantic.Bytes(), candidateSemantic.Bytes()) {
		t.Fatal("active carrier upgrade byte revision changed JSON semantics")
	}
	const attributePin = "crates/assets/data/block-item-routes-v*.json text eol=lf"
	baseAttributes := bytes.Replace(candidateAttributes, []byte(attributePin+attributeLineEnding), nil, 1)
	if bytes.Equal(baseAttributes, candidateAttributes) {
		t.Fatal("active carrier tracked-LF pin is absent")
	}
	var target bedrockTargetManifest
	if err := json.Unmarshal(candidateManifest, &target); err != nil {
		t.Fatal(err)
	}
	candidateDigest := fmt.Sprintf("%x", sha256.Sum256(candidateCarrier))
	if target.Hashes["block_item_routes"] != candidateDigest {
		t.Fatalf("candidate manifest hash %s does not match carrier %s", target.Hashes["block_item_routes"], candidateDigest)
	}
	baseDigest := fmt.Sprintf("%x", sha256.Sum256(baseCarrier))
	baseManifest := bytes.Replace(candidateManifest, []byte(candidateDigest), []byte(baseDigest), 1)
	if bytes.Equal(baseManifest, candidateManifest) {
		t.Fatal("candidate manifest does not contain the active carrier digest")
	}

	cloneRoot := filepath.Join(t.TempDir(), "upgrade")
	if output, err := exec.Command("git", "clone", "--quiet", "--shared", "--no-checkout", sourceRoot, cloneRoot).CombinedOutput(); err != nil {
		t.Fatalf("git clone failed: %v: %s", err, output)
	}
	runGit := func(arguments ...string) string {
		t.Helper()
		command := exec.Command("git", append([]string{"-C", cloneRoot}, arguments...)...)
		output, err := command.CombinedOutput()
		if err != nil {
			t.Fatalf("git %s failed: %v: %s", strings.Join(arguments, " "), err, output)
		}
		return strings.TrimSpace(string(output))
	}
	runGit("config", "core.autocrlf", "true")
	runGit("config", "user.name", "Cinnabar Test")
	runGit("config", "user.email", "cinnabar-test@example.invalid")
	runGit("checkout", "--quiet", "--detach", "HEAD")
	writeClone := func(path string, contents []byte) {
		t.Helper()
		if err := os.WriteFile(filepath.Join(cloneRoot, filepath.FromSlash(path)), contents, 0o644); err != nil {
			t.Fatal(err)
		}
	}
	writeClone(attributePath, baseAttributes)
	writeClone(carrierPath, baseCarrier)
	writeClone(manifestPath, baseManifest)
	runGit("add", "--", attributePath, carrierPath, manifestPath)
	runGit("commit", "--quiet", "-m", "synthetic pre-pin base")
	baseCommit := runGit("rev-parse", "HEAD")
	writeClone(attributePath, candidateAttributes)
	writeClone(carrierPath, candidateCarrier)
	writeClone(manifestPath, candidateManifest)
	runGit("add", "--", attributePath, carrierPath, manifestPath)
	runGit("commit", "--quiet", "-m", "synthetic candidate")
	candidateCommit := runGit("rev-parse", "HEAD")

	runGit("checkout", "--quiet", "--detach", baseCommit)
	baseCheckout, err := os.ReadFile(filepath.Join(cloneRoot, filepath.FromSlash(carrierPath)))
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Contains(baseCheckout, []byte("\r\n")) || fmt.Sprintf("%x", sha256.Sum256(baseCheckout)) == baseDigest {
		t.Fatal("synthetic autocrlf base did not reproduce rewritten checkout bytes")
	}
	runGit("checkout", "--quiet", "--detach", candidateCommit)
	candidateCheckout, err := os.ReadFile(filepath.Join(cloneRoot, filepath.FromSlash(carrierPath)))
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(candidateCheckout, candidateCarrier) {
		t.Fatalf("candidate checkout did not rematerialize exact tracked-LF carrier bytes: got %x, want %s", sha256.Sum256(candidateCheckout), candidateDigest)
	}
	command := exec.Command("go", "test", ".", "-run", "^TestBedrockTargetManifestOwnsEveryProductionCarrier$", "-count=1")
	command.Dir = filepath.Join(cloneRoot, "tools", "registrygen")
	if output, err := command.CombinedOutput(); err != nil {
		t.Fatalf("candidate target manifest test failed after upgrade: %v: %s", err, output)
	}
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
	for _, carrier := range []string{target.Artifacts["block_item_routes"]} {
		attributes, err := effectiveTrackedAttributes(root, carrier)
		if err != nil {
			t.Fatal(err)
		}
		if attributes["text"] != "set" || attributes["eol"] != "lf" {
			t.Fatalf("raw-byte target carrier %s is not pinned to tracked LF checkout bytes: %+v", carrier, attributes)
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
	workflow, err := os.ReadFile(filepath.Join(root, ".github", "workflows", "ci.yml"))
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(workflow), "make physics-assets") {
		t.Fatal("CI does not exercise the manifest-owned physics target")
	}
	for _, carrier := range []string{"block-physics-v1001", "block-physics-v2168"} {
		if strings.Contains(string(workflow), carrier) {
			t.Fatalf("CI hard-codes %s instead of using the manifest-owned Make target", carrier)
		}
	}
}
