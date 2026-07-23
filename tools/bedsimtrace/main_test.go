package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"reflect"
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
		Module                string `json:"module"`
		Version               string `json:"version"`
		SourceCommit          string `json:"source_commit"`
		ModuleSum             string `json:"module_sum"`
		GeneratorCommand      string `json:"generator_command"`
		GeneratorSourceSHA256 string `json:"generator_source_sha256"`
		GoModSHA256           string `json:"go_mod_sha256"`
		GoSumSHA256           string `json:"go_sum_sha256"`
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
	if got.Module != "github.com/oomph-ac/bedsim" || got.Version != "v0.1.3" || got.SourceCommit != "5be9149df14e30c0ab14f9e01d51dd2acfee5230" || got.ModuleSum != "h1:tWZ7O48DL/SaWIY+0zz0hFln+DXN4vfatqKr8zTHVo8=" || got.GeneratorCommand != "GOWORK=off go run . --terrain" {
		t.Fatalf("incomplete pinned module provenance: %#v", got)
	}
	assertFileHash(t, "main.go", got.GeneratorSourceSHA256)
	assertFileHash(t, "go.mod", got.GoModSHA256)
	assertFileHash(t, "go.sum", got.GoSumSHA256)
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
	if path == "main.go" || path == "go.mod" || path == "go.sum" {
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
		"ladder_ascend", "ladder_descend", "ladder_hold", "ladder_wall_climb", "water_enter",
		"water_swim", "water_exit", "lava", "cobweb", "slime_bounce",
		"slime_sneak", "slime_walk", "bed_bounce", "bed_sneak", "soul_sand",
		"honey", "scaffolding", "bubble_up", "bubble_down", "unloaded_boundary",
	}
	got := terrainScriptNames()
	if len(got) != len(want) {
		t.Fatalf("terrain script count = %d, want %d: %v", len(got), len(want), got)
	}
	seen := map[string]bool{}
	for _, name := range got {
		seen[name] = true
	}
	for _, name := range want {
		if !seen[name] {
			t.Fatalf("terrain script %q missing from %v", name, got)
		}
	}
}

func TestTerrainFixtureSeparatesObservedConformanceFromUnsupportedScripts(t *testing.T) {
	var output bytes.Buffer
	if err := writeTerrainTrace(&output); err != nil {
		t.Fatal(err)
	}
	seenScenarios := map[string]struct{}{}
	decoder := json.NewDecoder(&output)
	for decoder.More() {
		var script scenarioScript
		if err := decoder.Decode(&script); err != nil {
			t.Fatal(err)
		}
		if len(script.Steps) < 2 {
			t.Fatalf("%s is not a multi-tick script", script.Scenario)
		}
		if _, duplicate := seenScenarios[script.Scenario]; duplicate {
			t.Fatalf("duplicate scenario %q", script.Scenario)
		}
		seenScenarios[script.Scenario] = struct{}{}
		for _, step := range script.Steps {
			if script.Evidence.Status == "bedsim_observed_with_manifest_context" {
				if step.Expected == nil {
					t.Fatalf("observed %s step lacks complete result", script.Scenario)
				}
			} else if script.Evidence.Status != "unsupported_non_conformance" || script.Evidence.Reason == "" || step.Expected != nil {
				t.Fatalf("unsupported evidence was presented as conformance: %#v", script)
			}
		}
	}
	if len(seenScenarios) != len(terrainScriptNames()) {
		t.Fatalf("scenario count = %d, want %d", len(seenScenarios), len(terrainScriptNames()))
	}
}

func TestWaterScriptsEncodeActualEnvironmentTransitionsWithoutExpectedGoldens(t *testing.T) {
	for _, script := range terrainScripts() {
		if script.Scenario == "water_enter" {
			if script.Evidence.Status != "unsupported_non_conformance" || script.Steps[0].World.Physics.Flags&flagWater != 0 || script.Steps[1].World.Physics.Flags&flagWater == 0 {
				t.Fatalf("water enter transition is not explicit: %#v", script)
			}
			return
		}
	}
	t.Fatal("water_enter script missing")
}

func TestWorldIdentityBindsGeometryPhysicsCoordinatesAndRevision(t *testing.T) {
	base := terrainScripts()[0].Steps[0].World
	want := identity(base)
	mutations := []scenarioWorld{base, base, base, base}
	mutations[0].Boxes = append([]aabb{}, base.Boxes...)
	mutations[0].Boxes[0].Max.X++
	mutations[1].Physics.Friction = 0.7
	mutations[2].Origin[0] = 16
	mutations[3].Revision++
	for index, mutation := range mutations {
		if got := identity(mutation); reflect.DeepEqual(got, want) {
			t.Fatalf("identity mutation %d was not bound", index)
		}
	}
}
