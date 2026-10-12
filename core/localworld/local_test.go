package localworld

import (
	"context"
	"errors"
	"testing"
	"time"
)

var home = &World{ID: "0123456789abcdef", Name: "Home", GameMode: GameModeCreative, Backend: BackendBDS}

func running(target ConnectionTarget) Status {
	return Status{State: StateRunning, World: home, Target: &target}
}

func TestIdleFallsThroughAndRunningRoutesToItsTarget(t *testing.T) {
	local := NewLocal()
	if _, ok, err := local.ConnectionTarget(context.Background()); ok || err != nil {
		t.Fatalf("idle target = %v, %v", ok, err)
	}
	want := ConnectionTarget{Address: "127.0.0.1:5000", Transport: TransportNetherNetLAN, LANAddress: "127.0.0.1:7551", LevelName: home.ID}
	if _, err := local.SetLocal(running(want)); err != nil {
		t.Fatal(err)
	}
	got, ok, err := local.ConnectionTarget(context.Background())
	if err != nil || !ok || got != want {
		t.Fatalf("running target = %+v, %v, %v", got, ok, err)
	}
	world, isRunning, _ := local.Running()
	if !isRunning || world != *home {
		t.Fatalf("running world = %+v, %v", world, isRunning)
	}
}

// A failed or closing world must never fall through to another server.
func TestFailedAndClosingWorldsNeverFallThrough(t *testing.T) {
	local := NewLocal()
	for _, status := range []Status{
		{State: StateFailed, World: home, Error: "local world server failed to start"},
		{State: StateStopping, World: home},
	} {
		if _, err := local.SetLocal(status); err != nil {
			t.Fatal(err)
		}
		if _, ok, err := local.ConnectionTarget(context.Background()); ok || err == nil {
			t.Fatalf("%s: ok=%v err=%v", status.State, ok, err)
		}
		if _, isRunning, _ := local.Running(); isRunning {
			t.Fatalf("%s reported running", status.State)
		}
	}
}

func TestTargetWaitsForTheStartAndHonoursContext(t *testing.T) {
	local := NewLocal()
	if _, err := local.SetLocal(Status{State: StateStarting, World: home}); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()
	if _, _, err := local.ConnectionTarget(ctx); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("err = %v", err)
	}
	got := make(chan string, 1)
	go func() {
		target, _, _ := local.ConnectionTarget(context.Background())
		got <- target.Address
	}()
	if _, err := local.SetLocal(running(ConnectionTarget{Address: "127.0.0.1:1", Transport: TransportRakNet})); err != nil {
		t.Fatal(err)
	}
	select {
	case address := <-got:
		if address != "127.0.0.1:1" {
			t.Fatalf("address = %q", address)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("the target did not follow the start")
	}
}

// Opening supersedes other routes once per world: a start, not its own progress to running.
func TestOpeningIsReportedOncePerWorld(t *testing.T) {
	local := NewLocal()
	target := ConnectionTarget{Address: "127.0.0.1:1", Transport: TransportRakNet}
	for _, step := range []struct {
		status Status
		opened bool
	}{
		{Status{State: StateStarting, World: home}, true},
		{running(target), false},
		{Status{State: StateStopping, World: home}, false},
		{Status{State: StateIdle}, false},
		{running(target), true},
		{running(target), false},
	} {
		opened, err := local.SetLocal(step.status)
		if err != nil || opened != step.opened {
			t.Fatalf("%s: opened=%v err=%v, want %v", step.status.State, opened, err, step.opened)
		}
	}
	other := *home
	other.ID = "fedcba9876543210"
	if opened, _ := local.SetLocal(Status{State: StateStarting, World: &other}); !opened {
		t.Fatal("another world opening must supersede the route")
	}
}

func TestReportsThatCannotBeRoutedAreRejected(t *testing.T) {
	local := NewLocal()
	for _, status := range []Status{
		{State: "paused", World: home},
		{State: StateStarting},
		{State: StateRunning, World: home},
		running(ConnectionTarget{Transport: TransportRakNet}),
		running(ConnectionTarget{Address: "127.0.0.1:1", Transport: "quic"}),
	} {
		if _, err := local.SetLocal(status); !errors.Is(err, ErrInvalid) {
			t.Fatalf("%+v: err = %v", status, err)
		}
	}
	if _, ok, err := local.ConnectionTarget(context.Background()); ok || err != nil {
		t.Fatalf("a rejected report changed the route: %v %v", ok, err)
	}
}
