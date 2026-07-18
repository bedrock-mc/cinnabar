package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"strings"
	"testing"

	"github.com/oomph-ac/bedsim"
)

func TestBedsimInputEmitsStopSprintingOnTrueToFalseEdge(t *testing.T) {
	before := bedsim.MovementState{Sprinting: true}
	got := toBedsimInput(before, movementInput{Sprinting: false})
	if !got.StopSprinting {
		t.Fatal("true-to-false sprint edge did not emit StopSprinting")
	}
	if got.StartSprinting || got.SprintDown {
		t.Fatalf("stop edge also emitted start/down: %#v", got)
	}
}

func TestTerrainProvenanceBindsGeneratorScriptAndOutput(t *testing.T) {
	type provenance struct {
		GeneratorSourceSHA256 string `json:"generator_source_sha256"`
		ScriptSHA256          string `json:"script_sha256"`
		SHA256                string `json:"sha256"`
	}
	bytes, err := os.ReadFile("../../crates/sim/fixtures/bedsim-v0.1.3-terrain.provenance.json")
	if err != nil {
		t.Fatal(err)
	}
	var got provenance
	if err := json.Unmarshal(bytes, &got); err != nil {
		t.Fatal(err)
	}
	assertFileHash(t, "main.go", got.GeneratorSourceSHA256)
	script, err := json.Marshal(terrainScriptManifest())
	if err != nil {
		t.Fatal(err)
	}
	if actual := hashBytes(script); actual != got.ScriptSHA256 {
		t.Fatalf("script hash = %s, want %s", actual, got.ScriptSHA256)
	}
	assertFileHash(t, "../../crates/sim/fixtures/bedsim-v0.1.3-terrain.jsonl", got.SHA256)
}

func assertFileHash(t *testing.T, path, want string) {
	t.Helper()
	bytes, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if path == "main.go" {
		bytes = []byte(strings.ReplaceAll(string(bytes), "\r\n", "\n"))
	}
	if got := hashBytes(bytes); got != want {
		t.Fatalf("%s hash = %s, want %s", path, got, want)
	}
}

func hashBytes(bytes []byte) string {
	digest := sha256.Sum256(bytes)
	return hex.EncodeToString(digest[:])
}

func TestGeneratedTraceMatchesPinnedFixtureExactly(t *testing.T) {
	want, err := os.ReadFile("../../crates/sim/fixtures/bedsim-v0.1.3-basic.jsonl")
	if err != nil {
		t.Fatalf("read pinned fixture: %v", err)
	}
	var got bytes.Buffer
	if err := writeTrace(&got); err != nil {
		t.Fatalf("write trace: %v", err)
	}
	if !bytes.Equal(got.Bytes(), want) {
		t.Fatalf("generated trace differs from pinned fixture: got %d bytes, want %d", got.Len(), len(want))
	}
}

func TestGeneratedTerrainTraceMatchesPinnedFixtureExactly(t *testing.T) {
	want, err := os.ReadFile("../../crates/sim/fixtures/bedsim-v0.1.3-terrain.jsonl")
	if err != nil {
		t.Fatalf("read pinned terrain fixture: %v", err)
	}
	var got bytes.Buffer
	if err := writeTerrainTrace(&got); err != nil {
		t.Fatalf("write terrain trace: %v", err)
	}
	if !bytes.Equal(got.Bytes(), want) {
		t.Fatalf("generated terrain trace differs from pinned fixture: got %d bytes, want %d", got.Len(), len(want))
	}
}

func TestTerrainScriptsCoverEveryTaskThreeStratum(t *testing.T) {
	want := []string{
		"flat_walk", "diagonal", "sprint_jump", "slab_step", "stair_step",
		"sneak_north", "sneak_south", "sneak_east", "sneak_west", "head_collision",
		"ladder_ascend", "ladder_descend", "ladder_hold", "water_enter", "water_swim",
		"water_exit", "lava", "cobweb", "slime_bounce", "slime_sneak",
		"bed_bounce", "soul_sand", "honey", "scaffolding", "bubble_up",
		"bubble_down", "unloaded_boundary",
	}
	got := terrainScriptNames()
	if len(got) != len(want) {
		t.Fatalf("terrain script count = %d, want %d: %v", len(got), len(want), got)
	}
	for index := range want {
		if got[index] != want[index] {
			t.Fatalf("terrain script %d = %q, want %q", index, got[index], want[index])
		}
	}
}

func TestTerrainFixtureContainsDistinctWorldBoundCompleteScenarios(t *testing.T) {
	var output bytes.Buffer
	if err := writeTerrainTrace(&output); err != nil {
		t.Fatal(err)
	}
	seenScenarios := map[string]struct{}{}
	seenWorlds := map[string]struct{}{}
	decoder := json.NewDecoder(&output)
	for decoder.More() {
		var record map[string]any
		if err := decoder.Decode(&record); err != nil {
			t.Fatal(err)
		}
		scenario, scenarioOK := record["scenario"].(string)
		world, worldOK := record["world"].(map[string]any)
		worldName, worldNameOK := world["name"].(string)
		expected, expectedOK := record["expected"].(map[string]any)
		environmentOK, identityOK := false, false
		if expectedOK {
			_, environmentOK = expected["environment"].(map[string]any)
			_, identityOK = expected["world_identity"].(map[string]any)
		}
		_, errorOK := record["expected_error"].(string)
		if !scenarioOK || !worldOK || !worldNameOK || !(expectedOK && environmentOK && identityOK || errorOK) {
			t.Fatalf("incomplete scenario record: %#v", record)
		}
		if _, duplicate := seenScenarios[scenario]; duplicate {
			t.Fatalf("duplicate scenario %q", scenario)
		}
		if _, duplicate := seenWorlds[worldName]; duplicate {
			t.Fatalf("duplicate world %q", worldName)
		}
		seenScenarios[scenario] = struct{}{}
		seenWorlds[worldName] = struct{}{}
	}
	if len(seenScenarios) != len(terrainScriptNames()) {
		t.Fatalf("scenario count = %d, want %d", len(seenScenarios), len(terrainScriptNames()))
	}
}
