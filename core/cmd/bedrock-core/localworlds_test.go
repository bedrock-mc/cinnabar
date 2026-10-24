package main

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/control"
	"github.com/hashimthearab/rust-mcbe/core/localworld"
)

// The open hook runs once per world that begins opening, and rejected reports reach control classified.
func TestControlWorldsRunTheOpenHookOncePerOpenedWorld(t *testing.T) {
	local := localworld.NewLocal()
	opened := 0
	worlds := controlWorlds{local: local, onOpen: func() { opened++ }}
	home := &control.LocalWorldInfo{ID: "0123456789abcdef", Name: "Home", GameMode: "survival", Backend: "bds"}
	if err := worlds.SetLocal(control.LocalWorld{State: "starting", World: home}); err != nil || opened != 1 {
		t.Fatalf("SetLocal(starting) = %v, hook calls %d", err, opened)
	}
	running := control.LocalWorld{State: "running", World: home, Target: &control.LocalWorldTarget{Address: "127.0.0.1:5000", Transport: "nethernet"}}
	if err := worlds.SetLocal(running); err != nil || opened != 1 {
		t.Fatalf("SetLocal(running) = %v, hook calls %d", err, opened)
	}
	target, ok, err := local.ConnectionTarget(context.Background())
	if err != nil || !ok || target.Address != "127.0.0.1:5000" || target.Transport != localworld.TransportNetherNetHTTP {
		t.Fatalf("route = %+v, %v, %v", target, ok, err)
	}
	if err := worlds.SetLocal(control.LocalWorld{State: "running", World: home}); !errors.Is(err, control.ErrWorldInvalid) || opened != 1 {
		t.Fatalf("SetLocal(no target) = %v, hook calls %d", err, opened)
	}
	if err := worlds.Invite(context.Background(), "2535400000000000"); !errors.Is(err, control.ErrWorldNotOpen) {
		t.Fatalf("Invite() without hosting = %v", err)
	}
}

func TestLocalWorldReportPreservesMultiplayerDenial(t *testing.T) {
	bytes, err := os.ReadFile("../../../crates/bridge/data/local-world-single-player.json")
	if err != nil {
		t.Fatal(err)
	}
	var report control.LocalWorld
	if err := json.Unmarshal(bytes, &report); err != nil {
		t.Fatal(err)
	}
	status := localStatus(report)
	if status.World == nil || !status.World.MultiplayerDisabled {
		t.Fatal("multiplayer denial was lost")
	}
}
