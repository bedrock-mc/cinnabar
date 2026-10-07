package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"github.com/hashimthearab/rust-mcbe/tools/registrygen/internal/targetpin"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

const validBlockedFoundation = `{
  "schema": "cinnabar.registry-foundation.v1",
  "status": "blocked",
  "game_version": "1.26.50",
  "protocol": 2193,
  "formats": {"block": "BREG1003", "light": "LREG1001", "biome": "BIOREG01"},
  "outputs": {
    "block": "crates/assets/data/block-registry-v2193.bin",
    "light": "crates/assets/data/block-light-registry-v2193.bin",
    "biome": "crates/assets/data/biome-registry-v2193.bin"
  },
  "sources": {
    "dragonfly": {
      "commit": "4c7b5074be94fa83a1cd98e9c752083ad04a6e21",
      "blob": "ee29e5e039086c10bdfb964621a8e146b4f7af19",
      "sha256": "f0784a6284d6ca7d98cc3472f4ce84241a11e11b18ed16f6591dfd5e6da6fbd6",
      "size": 3102889
    },
    "bds": {
      "archive_sha256": "2c9b98d07d2504786996f2335980e88bd969b4a77514925e75471a1349995825",
      "executable_sha256": "19c88569af2e4b7d984e999055a31cbcb0799dacf8bbbf7371eda42f5772a443",
      "overlay_sha256": "f7cc20dd63cc799381368b55104d8a7b7dd20fc654a655a2188833db9f663339"
    }
  },
  "missing": [
    "retail_block_projection",
    "authoritative_light_projection"
  ],
  "projection_bindings": {
    "biome": {"sha256": "e3ba3d96a66fa49b3b7d94ae6b67b4cc5d8961789c91275080d3922909b25c2a"}
  }
}`

func TestRegistryFoundationAcceptsExactBlockedEvidence(t *testing.T) {
	result, err := ValidateRegistryFoundation(strings.NewReader(validBlockedFoundation))
	if err != nil {
		t.Fatalf("validate blocked foundation: %v", err)
	}
	if result.Status != FoundationBlocked {
		t.Fatalf("status = %q, want %q", result.Status, FoundationBlocked)
	}
	want := []MissingProjection{
		MissingRetailBlockProjection,
		MissingAuthoritativeLightProjection,
	}
	if strings.Join(result.MissingStrings(), ",") != strings.Join(missingStrings(want), ",") {
		t.Fatalf("missing = %v, want %v", result.Missing, want)
	}
}

func TestRegistryFoundationRejectsMalformedAndTrailingJSON(t *testing.T) {
	for name, input := range map[string]string{
		"malformed": `{`,
		"unknown":   strings.Replace(validBlockedFoundation, `"schema":`, `"extra": true, "schema":`, 1),
		"trailing":  validBlockedFoundation + ` {}`,
	} {
		t.Run(name, func(t *testing.T) {
			if _, err := ValidateRegistryFoundation(strings.NewReader(input)); err == nil {
				t.Fatal("accepted invalid JSON")
			}
		})
	}
}

func TestRegistryFoundationRejectsVersionHashMagicAndLegacyOutputs(t *testing.T) {
	tests := map[string]string{
		"game version":   strings.Replace(validBlockedFoundation, `"1.26.50"`, `"1.26.51"`, 1),
		"protocol":       strings.Replace(validBlockedFoundation, `2193`, `2192`, 1),
		"uppercase hash": strings.Replace(validBlockedFoundation, `f0784a62`, `F0784A62`, 1),
		"short hash":     strings.Replace(validBlockedFoundation, `f0784a6284d6ca7d98cc3472f4ce84241a11e11b18ed16f6591dfd5e6da6fbd6`, `abcd`, 1),
		"block magic":    strings.Replace(validBlockedFoundation, `BREG1003`, `BREG1002`, 1),
		"light magic":    strings.Replace(validBlockedFoundation, `LREG1001`, `LREG1002`, 1),
		"biome magic":    strings.Replace(validBlockedFoundation, `BIOREG01`, `BIOREG02`, 1),
		"legacy output":  strings.Replace(validBlockedFoundation, `block-registry-v2193.bin`, `block-registry-v1001.bin`, 1),
	}
	for name, input := range tests {
		t.Run(name, func(t *testing.T) {
			if _, err := ValidateRegistryFoundation(strings.NewReader(input)); err == nil {
				t.Fatal("accepted invalid foundation")
			}
		})
	}
}

func TestRegistryFoundationRejectsForbiddenRegistrySurface(t *testing.T) {
	forbidden := "P" + "REG"
	input := strings.Replace(validBlockedFoundation, `"block": "BREG1003"`, `"block": "BREG1003", "`+forbidden+`": "`+forbidden+`1001"`, 1)
	if _, err := ValidateRegistryFoundation(strings.NewReader(input)); err == nil {
		t.Fatal("accepted forbidden registry field")
	}
}

func TestRegistryFoundationReadyRequiresThreeSeparatelyBoundProjections(t *testing.T) {
	ready := strings.Replace(validBlockedFoundation, `"status": "blocked"`, `"status": "ready"`, 1)
	ready = strings.Replace(ready, `,
  "missing": [
    "retail_block_projection",
    "authoritative_light_projection"
  ],
  "projection_bindings": {
    "biome": {"sha256": "e3ba3d96a66fa49b3b7d94ae6b67b4cc5d8961789c91275080d3922909b25c2a"}
  }`, ``, 1)
	if _, err := ValidateRegistryFoundation(strings.NewReader(ready)); err == nil {
		t.Fatal("accepted ready foundation without projection bindings")
	}
	ready = validReadyFoundation()
	result, err := ValidateRegistryFoundation(strings.NewReader(ready))
	if err != nil {
		t.Fatalf("reject separately bound ready foundation: %v", err)
	}
	if result.Status != FoundationReady || len(result.Missing) != 0 {
		t.Fatalf("ready result = %#v", result)
	}
	wrongBiome := strings.Replace(ready,
		"e3ba3d96a66fa49b3b7d94ae6b67b4cc5d8961789c91275080d3922909b25c2a",
		"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", 1)
	if _, err := ValidateRegistryFoundation(strings.NewReader(wrongBiome)); err == nil {
		t.Fatal("accepted ready foundation with a different biome projection binding")
	}
	blockHash, err := targetpin.BlockHash()
	if err != nil {
		t.Fatal(err)
	}
	wrongBlock := strings.Replace(ready, blockHash,
		"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", 1)
	if _, err := ValidateRegistryFoundation(strings.NewReader(wrongBlock)); err == nil {
		t.Fatal("accepted ready foundation with a different block projection binding")
	}
}

func TestRegistryFoundationValidationCreatesNoOutputs(t *testing.T) {
	root := filepath.Join("..", "..")
	outputs := []string{
		"crates/assets/data/block-registry-v2193.bin",
		"crates/assets/data/block-light-registry-v2193.bin",
	}
	before := make(map[string][]byte, len(outputs))
	for _, output := range outputs {
		payload, err := os.ReadFile(filepath.Join(root, filepath.FromSlash(output)))
		if err != nil {
			t.Fatalf("read checked output %s: %v", output, err)
		}
		before[output] = payload
	}
	if _, err := ValidateRegistryFoundation(strings.NewReader(validBlockedFoundation)); err != nil {
		t.Fatalf("validate: %v", err)
	}
	for _, output := range outputs {
		payload, err := os.ReadFile(filepath.Join(root, filepath.FromSlash(output)))
		if err != nil || !bytes.Equal(payload, before[output]) {
			t.Fatalf("validation changed output: %s", output)
		}
	}
}

func TestRegistryFoundationCommandExitContract(t *testing.T) {
	binary := filepath.Join(t.TempDir(), "foundationcheck")
	if runtime.GOOS == "windows" {
		binary += ".exe"
	}
	build := exec.Command("go", "build", "-o", binary, "./cmd/foundationcheck")
	if output, err := build.CombinedOutput(); err != nil {
		t.Fatalf("build foundationcheck: %v\n%s", err, output)
	}
	dir := t.TempDir()
	blocked := filepath.Join("..", "..", "assets", "registry-foundation-v2193.json")
	ready := filepath.Join(dir, "ready.json")
	malformed := filepath.Join(dir, "malformed.json")
	if err := os.WriteFile(ready, []byte(validReadyFoundation()), 0o600); err != nil {
		t.Fatalf("write ready fixture: %v", err)
	}
	if err := os.WriteFile(malformed, []byte(`{`), 0o600); err != nil {
		t.Fatalf("write malformed fixture: %v", err)
	}
	missing := filepath.Join(dir, "missing.json")
	tests := []struct {
		name string
		args []string
		want int
	}{
		{name: "checked ready", args: []string{"-manifest", blocked}, want: 0},
		{name: "checked ready expected blocked", args: []string{"-manifest", blocked, "-expect-blocked"}, want: 1},
		{name: "valid ready", args: []string{"-manifest", ready}, want: 0},
		{name: "valid ready expected blocked", args: []string{"-manifest", ready, "-expect-blocked"}, want: 1},
	}
	invalid := []struct {
		name string
		args []string
	}{
		{name: "malformed", args: []string{"-manifest", malformed}},
		{name: "missing path", args: []string{"-manifest", missing}},
		{name: "unknown flag", args: []string{"-unknown"}},
		{name: "missing value", args: []string{"-manifest"}},
		{name: "extra positional", args: []string{"-manifest", blocked, "extra"}},
	}
	for _, test := range invalid {
		tests = append(tests,
			struct {
				name string
				args []string
				want int
			}{name: test.name, args: test.args, want: 1},
			struct {
				name string
				args []string
				want int
			}{name: test.name + " with expect", args: append([]string{"-expect-blocked"}, test.args...), want: 1},
		)
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			command := exec.Command(binary, test.args...)
			output, err := command.CombinedOutput()
			if got := commandExitCode(err); got != test.want {
				t.Fatalf("exit = %d, want %d; output=%s", got, test.want, output)
			}
		})
	}
}

func TestRegistryFoundationMakeTargetIsIsolatedAndReady(t *testing.T) {
	makefile, err := os.ReadFile(filepath.Join("..", "..", "Makefile"))
	if err != nil {
		t.Fatalf("read Makefile: %v", err)
	}
	text := string(makefile)
	for _, required := range []string{
		".DEFAULT_GOAL := help",
		"REGISTRY_FOUNDATION_MANIFEST ?= assets/registry-foundation-v2193.json",
		"registry-foundation-check:",
		"Validate the exact protocol-2193 registry foundation",
	} {
		if !strings.Contains(text, required) {
			t.Fatalf("Makefile missing %q", required)
		}
	}
	if bytes.Contains(makefile, []byte("P"+"REG")) {
		t.Fatal("Makefile introduced a forbidden registry foundation string")
	}
	var foundationLines []string
	for _, line := range strings.Split(text, "\n") {
		if strings.Contains(line, "REGISTRY_FOUNDATION") || strings.HasPrefix(line, "registry-foundation-check:") {
			foundationLines = append(foundationLines, line)
		}
	}
	if strings.Contains(strings.ToLower(strings.Join(foundationLines, "\n")), "phy"+"sics") {
		t.Fatal("foundation target references an unrelated registry")
	}
	assetsLine := makeTargetLine(t, text, "assets:")
	clientLine := makeTargetLine(t, text, "client:")
	for _, line := range []string{assetsLine, clientLine} {
		if strings.Contains(line, "registry-foundation") {
			t.Fatalf("foundation leaked into a default dependency: %s", line)
		}
	}
}

func missingStrings(values []MissingProjection) []string {
	result := make([]string, len(values))
	for i, value := range values {
		result[i] = string(value)
	}
	return result
}

// validReadyFoundation binds the fixture to the current target carrier identities.
func validReadyFoundation() string {
	blockHash, err := targetpin.BlockHash()
	if err != nil {
		panic(err)
	}
	lightHash, err := targetpin.LightHash()
	if err != nil {
		panic(err)
	}
	ready := strings.Replace(validBlockedFoundation, `"status": "blocked"`, `"status": "ready"`, 1)
	ready = strings.Replace(ready, `,
  "missing": [
    "retail_block_projection",
    "authoritative_light_projection"
  ],
  "projection_bindings": {
    "biome": {"sha256": "e3ba3d96a66fa49b3b7d94ae6b67b4cc5d8961789c91275080d3922909b25c2a"}
  }`, `,
  "projection_bindings": {
    "block": {"sha256": "`+blockHash+`"},
    "biome": {"sha256": "e3ba3d96a66fa49b3b7d94ae6b67b4cc5d8961789c91275080d3922909b25c2a"},
    "light": {"sha256": "`+lightHash+`"}
  }`, 1)
	return ready
}

func commandExitCode(err error) int {
	if err == nil {
		return 0
	}
	var exitError *exec.ExitError
	if errors.As(err, &exitError) {
		return exitError.ExitCode()
	}
	return -1
}

func makeTargetLine(t *testing.T, text, target string) string {
	t.Helper()
	for _, line := range strings.Split(text, "\n") {
		if strings.HasPrefix(line, target) {
			return line
		}
	}
	t.Fatalf("missing Make target %q", target)
	return ""
}

// TestRegistryFoundationFollowsTargetBlockPin rejects stale bindings after a manifest update.
func TestRegistryFoundationFollowsTargetBlockPin(t *testing.T) {
	ready := validReadyFoundation()
	original, err := targetpin.BlockHash()
	if err != nil {
		t.Fatal(err)
	}
	light, err := targetpin.LightHash()
	if err != nil {
		t.Fatal(err)
	}
	changed := strings.Repeat("a", 64)
	dir := t.TempDir()
	if err := os.Mkdir(filepath.Join(dir, "assets"), 0755); err != nil {
		t.Fatal(err)
	}
	target, err := json.Marshal(map[string]any{"hashes": map[string]string{"block_registry": changed, "light_registry": light}})
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "assets", "bedrock-target.json"), target, 0644); err != nil {
		t.Fatal(err)
	}
	t.Chdir(dir)
	if _, err := ValidateRegistryFoundation(strings.NewReader(strings.Replace(ready, original, changed, 1))); err != nil {
		t.Fatalf("rejected current target block binding: %v", err)
	}
	if _, err := ValidateRegistryFoundation(strings.NewReader(ready)); err == nil {
		t.Fatal("accepted stale target block binding")
	}
}
