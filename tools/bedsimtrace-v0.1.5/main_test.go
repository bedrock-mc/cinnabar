package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"strings"
	"testing"
)

func TestGeneratedTraceMatchesPinnedFixtureExactly(t *testing.T) {
	want, err := os.ReadFile("../../crates/sim/fixtures/bedsim-v0.1.5-liquid.jsonl")
	if err != nil {
		t.Fatal(err)
	}
	var first, second bytes.Buffer
	if err := writeTrace(&first); err != nil {
		t.Fatal(err)
	}
	if err := writeTrace(&second); err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(first.Bytes(), second.Bytes()) {
		t.Fatal("two v0.1.5 liquid generations differ")
	}
	if !bytes.Equal(first.Bytes(), want) {
		t.Fatalf("generated trace differs from fixture: got %d bytes, want %d", first.Len(), len(want))
	}
}

func TestLiquidTraceCoversOpenWaterAndLedgeExitControls(t *testing.T) {
	var output bytes.Buffer
	if err := writeTrace(&output); err != nil {
		t.Fatal(err)
	}
	want := map[string]float64{
		"water_ledge_exit_boost":           0.30000001192092896,
		"water_ledge_exit_blocked_above":   0.027000002562999725,
		"water_ledge_exit_still_submerged": 0.027000002562999725,
		"open_water_held_ascent":           0.027000002562999725,
	}
	for _, line := range strings.Split(strings.TrimSuffix(output.String(), "\n"), "\n") {
		var script scenarioScript
		if err := json.Unmarshal([]byte(line), &script); err != nil {
			t.Fatal(err)
		}
		expected, ok := want[script.Scenario]
		if !ok {
			t.Fatalf("unexpected scenario %q", script.Scenario)
		}
		switch script.Scenario {
		case "water_ledge_exit_boost", "water_ledge_exit_blocked_above", "water_ledge_exit_still_submerged":
			if len(script.Steps) != 1 || !script.Steps[0].Input.Jumping || !script.Steps[0].Expected.Collisions.X {
				t.Fatalf("%s is not a held-ascent ledge collision", script.Scenario)
			}
			if got := script.Steps[0].Expected.Velocity.Y; got != expected {
				t.Fatalf("%s vertical velocity = %v, want %v", script.Scenario, got, expected)
			}
		case "open_water_held_ascent":
			if len(script.Steps) != 1 || !script.Steps[0].Input.Jumping || script.Steps[0].Expected.Collisions.X || script.Steps[0].Expected.Collisions.Z || !script.Steps[0].Expected.Environment.InWater {
				t.Fatalf("open-water ascent is not an unblocked submerged held ascent: %#v", script)
			}
			if got := script.Steps[0].Expected.Velocity.Y; got != expected {
				t.Fatalf("open-water ascent velocity = %v, want %v", got, expected)
			}
		}
		delete(want, script.Scenario)
	}
	if len(want) != 0 {
		t.Fatalf("missing liquid scenarios: %v", want)
	}
}

func TestLiquidProvenanceBindsPinnedModuleGeneratorAndOutput(t *testing.T) {
	type provenance struct {
		Module                string `json:"module"`
		Version               string `json:"version"`
		SourceCommit          string `json:"source_commit"`
		ModuleSum             string `json:"module_sum"`
		Generator             string `json:"generator"`
		GeneratorCommand      string `json:"generator_command"`
		GeneratorSourceSHA256 string `json:"generator_source_sha256"`
		GoModSHA256           string `json:"go_mod_sha256"`
		GoSumSHA256           string `json:"go_sum_sha256"`
		SHA256                string `json:"sha256"`
	}
	bytes, err := os.ReadFile("../../crates/sim/fixtures/bedsim-v0.1.5-liquid.provenance.json")
	if err != nil {
		t.Fatal(err)
	}
	var got provenance
	if err := json.Unmarshal(bytes, &got); err != nil {
		t.Fatal(err)
	}
	if got.Module != "github.com/oomph-ac/bedsim" || got.Version != "v0.1.5" || got.SourceCommit != "f6a0e6bdf72cf3e735198e3695086d59da456d79" || got.ModuleSum != "h1:LCAA1aK65z9TBkFOY4tv6qkkTXxXK+NxJeOz/SyUSd8=" || got.Generator != "tools/bedsimtrace-v0.1.5" || got.GeneratorCommand != "GOWORK=off go run ." {
		t.Fatalf("incomplete v0.1.5 provenance: %#v", got)
	}
	assertFileHash(t, "main.go", got.GeneratorSourceSHA256)
	assertFileHash(t, "go.mod", got.GoModSHA256)
	assertFileHash(t, "go.sum", got.GoSumSHA256)
	assertFileHash(t, "../../crates/sim/fixtures/bedsim-v0.1.5-liquid.jsonl", got.SHA256)
}

// assertFileHash compares a file's SHA-256 after normalising source line endings.
func assertFileHash(t *testing.T, path, want string) {
	t.Helper()
	bytes, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if path == "main.go" || path == "go.mod" || path == "go.sum" {
		bytes = []byte(strings.ReplaceAll(string(bytes), "\r\n", "\n"))
	}
	if got := hashBytes(bytes); got != want {
		t.Fatalf("%s hash = %s, want %s", path, got, want)
	}
}

// hashBytes returns the lowercase SHA-256 hex encoding used by provenance.
func hashBytes(bytes []byte) string {
	digest := sha256.Sum256(bytes)
	return hex.EncodeToString(digest[:])
}
