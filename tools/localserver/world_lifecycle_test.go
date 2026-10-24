package main

import (
	"testing"

	"github.com/df-mc/dragonfly/server/world"
)

// TestWorldLifecycleInheritsPause covers worlds admitted during a pause and released before close.
func TestWorldLifecycleInheritsPause(t *testing.T) {
	p := &pauseState{settings: settings{gameMode: "survival", diff: "peaceful"}}
	startup := world.Config{Synchronous: true}.New()
	runtime := world.Config{Synchronous: true}.New()
	defer startup.Close()
	defer runtime.Close()
	p.WorldOpened(startup)
	p.request(true)
	p.WorldOpened(runtime)
	if !startup.Paused() || !runtime.Paused() {
		t.Fatal("world created during pause kept simulating")
	}
	p.request(false)
	if startup.Paused() || runtime.Paused() {
		t.Fatal("resume did not reach every live world")
	}
	p.WorldClosed(runtime)
	p.request(true)
	if !startup.Paused() || runtime.Paused() {
		t.Fatal("closed world retained pause ownership")
	}
}

// TestWorldLifecycleAppliesSelectedSettings uses the same gameplay initializer for every opening.
func TestWorldLifecycleAppliesSelectedSettings(t *testing.T) {
	for _, cfg := range []settings{
		{gameMode: "creative", diff: "peaceful"},
		{gameMode: "adventure", diff: "hard", terrainFixture: true},
		{gameMode: "survival", diff: "easy", opaqueOverdraw: true},
	} {
		p := &pauseState{settings: cfg}
		for _, dim := range []world.Dimension{world.Overworld, world.Nether, world.End} {
			w := world.Config{Synchronous: true, Dim: dim}.New()
			p.WorldOpened(w)
			mode, _ := cfg.worldGameMode()
			difficulty, _ := cfg.worldDifficulty()
			if w.DefaultGameMode() != mode || w.Difficulty() != difficulty {
				t.Error("opening ignored selected gameplay settings")
			}
			if (cfg.terrainFixture || cfg.opaqueOverdraw) && w.TimeCycle() {
				t.Error("opening ignored fixture time settings")
			}
			p.WorldClosed(w)
			if err := w.Close(); err != nil {
				t.Fatal(err)
			}
		}
	}
}
