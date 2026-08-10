package main

import (
	"bytes"
	"errors"
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
  "game_version": "1.26.40",
  "protocol": 2168,
  "formats": {"block": "BREG1003", "light": "LREG1001", "biome": "BIOREG01"},
  "outputs": {
    "block": "crates/assets/data/block-registry-v2168.bin",
    "light": "crates/assets/data/block-light-registry-v2168.bin",
    "biome": "crates/assets/data/biome-registry-v2168.bin"
  },
  "sources": {
    "dragonfly": {
      "commit": "0c2c404540fc651873c24a020b0a48778bd56295",
      "blob": "7006d9d46217425aab8e7d998f70c370b6b9c4eb",
      "sha256": "1dc6d7ea26b48b5b5e4702762e463b95e59eb109f26c0c3b74115d12cb1941a7",
      "size": 2436125
    },
    "bds": {
      "archive_sha256": "7b649671e1d88f8bd1499c580910f099e27533efc213f9faf5a5c68dd41a77c9",
      "executable_sha256": "e7775e636b9fdcbc354823d92d0c22c12738a2141d12557d856744293d258372",
      "overlay_sha256": "c52bbdfa8c92679595b5e342bee556a891a8aab91d5173f8670ff15e47e3efbb"
    }
  },
  "missing": [
    "retail_block_projection",
    "numeric_biome_projection",
    "authoritative_light_projection"
  ]
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
		MissingNumericBiomeProjection,
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
		"game version":   strings.Replace(validBlockedFoundation, `"1.26.40"`, `"1.26.41"`, 1),
		"protocol":       strings.Replace(validBlockedFoundation, `2168`, `2167`, 1),
		"uppercase hash": strings.Replace(validBlockedFoundation, `1dc6d7ea`, `1DC6D7EA`, 1),
		"short hash":     strings.Replace(validBlockedFoundation, `1dc6d7ea26b48b5b5e4702762e463b95e59eb109f26c0c3b74115d12cb1941a7`, `abcd`, 1),
		"block magic":    strings.Replace(validBlockedFoundation, `BREG1003`, `BREG1002`, 1),
		"light magic":    strings.Replace(validBlockedFoundation, `LREG1001`, `LREG1002`, 1),
		"biome magic":    strings.Replace(validBlockedFoundation, `BIOREG01`, `BIOREG02`, 1),
		"legacy output":  strings.Replace(validBlockedFoundation, `block-registry-v2168.bin`, `block-registry-v1001.bin`, 1),
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
    "numeric_biome_projection",
    "authoritative_light_projection"
  ]`, ``, 1)
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
}

func TestRegistryFoundationValidationCreatesNoOutputs(t *testing.T) {
	root := filepath.Join("..", "..")
	outputs := []string{
		"crates/assets/data/block-registry-v2168.bin",
		"crates/assets/data/block-light-registry-v2168.bin",
		"crates/assets/data/biome-registry-v2168.bin",
	}
	for _, output := range outputs {
		if _, err := os.Stat(filepath.Join(root, filepath.FromSlash(output))); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("output unexpectedly exists before validation: %s", output)
		}
	}
	if _, err := ValidateRegistryFoundation(strings.NewReader(validBlockedFoundation)); err != nil {
		t.Fatalf("validate: %v", err)
	}
	for _, output := range outputs {
		if _, err := os.Stat(filepath.Join(root, filepath.FromSlash(output))); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("validation created output: %s", output)
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
	blocked := filepath.Join("..", "..", "assets", "registry-foundation-v2168.json")
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
		{name: "valid blocked", args: []string{"-manifest", blocked}, want: 2},
		{name: "valid blocked expected", args: []string{"-manifest", blocked, "-expect-blocked"}, want: 0},
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

func TestRegistryFoundationMakeTargetIsIsolatedAndBlocked(t *testing.T) {
	makefile, err := os.ReadFile(filepath.Join("..", "..", "Makefile"))
	if err != nil {
		t.Fatalf("read Makefile: %v", err)
	}
	text := string(makefile)
	for _, required := range []string{
		".DEFAULT_GOAL := help",
		"REGISTRY_FOUNDATION_MANIFEST ?= assets/registry-foundation-v2168.json",
		"registry-foundation-check:",
		"-expect-blocked",
		"BLOCK_REGISTRY ?= crates/assets/data/block-registry-v1001.bin",
		"LIGHT_REGISTRY ?= crates/assets/data/block-light-registry-v1001.bin",
		"BIOME_REGISTRY ?= crates/assets/data/biome-registry-v1001.bin",
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

func validReadyFoundation() string {
	ready := strings.Replace(validBlockedFoundation, `"status": "blocked"`, `"status": "ready"`, 1)
	ready = strings.Replace(ready, `,
  "missing": [
    "retail_block_projection",
    "numeric_biome_projection",
    "authoritative_light_projection"
  ]`, ``, 1)
	bindings := `,
  "projection_bindings": {
    "block": {"sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
    "biome": {"sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"},
    "light": {"sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"}
  }`
	return strings.TrimSuffix(ready, "\n}") + bindings + "\n}"
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
