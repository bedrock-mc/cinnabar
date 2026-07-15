package main

import (
	"bytes"
	"os"
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
