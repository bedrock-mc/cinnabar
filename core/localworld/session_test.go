package localworld

import (
	"context"
	"errors"
	"testing"
)

// A session lookup follows its own route updates but never a replacement opening.
func TestSessionTargetIsBoundToItsOpening(t *testing.T) {
	for _, change := range []string{"another world", "reopen", "restart", "metadata"} {
		t.Run(change, func(t *testing.T) {
			local := NewLocal()
			first := ConnectionTarget{Address: "127.0.0.1:5000", Transport: TransportRakNet}
			set := func(status Status) {
				t.Helper()
				if _, err := local.SetLocal(status); err != nil {
					t.Fatal(err)
				}
			}
			set(running(first))
			session, ok, changed := local.RunningSession()
			if !ok || !session.Current() {
				t.Fatal("running session is not current")
			}
			updated := first
			updated.Address = "127.0.0.1:6000"
			set(running(updated))
			if target, ok, err := session.ConnectionTarget(context.Background()); err != nil || !ok || target != updated {
				t.Fatalf("same opening's updated route = %+v, %v, %v", target, ok, err)
			}
			world := *home
			switch change {
			case "another world":
				world.ID = "fedcba9876543210"
			case "reopen":
				set(Status{State: StateIdle})
			case "restart":
				set(Status{State: StateStarting, World: &world})
			case "metadata":
				world.Name = "Renamed world"
			}
			set(Status{State: StateRunning, World: &world, Target: &updated})
			if session.Current() {
				t.Fatal("superseded session is still current")
			}
			if _, ok, err := session.ConnectionTarget(context.Background()); ok || !errors.Is(err, ErrNotOpen) {
				t.Fatalf("superseded session routed to replacement: ok=%v err=%v", ok, err)
			}
			select {
			case <-changed:
			default:
				t.Fatal("the original change notification was not closed")
			}
			next, ok, _ := local.RunningSession()
			if !ok || next == session || !next.Current() || next.World != world {
				t.Fatal("replacement session was not captured")
			}
		})
	}
}

// A stopped session refuses routing immediately without waiting for a later running report.
func TestSessionTargetRefusesClosedWorldAndCancelledCaller(t *testing.T) {
	local := NewLocal()
	if _, err := local.SetLocal(running(ConnectionTarget{Address: "127.0.0.1:5000", Transport: TransportRakNet})); err != nil {
		t.Fatal(err)
	}
	session, _, _ := local.RunningSession()
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, ok, err := session.ConnectionTarget(ctx); ok || !errors.Is(err, context.Canceled) {
		t.Fatalf("cancelled lookup: ok=%v err=%v", ok, err)
	}
	if _, err := local.SetLocal(Status{State: StateStopping, World: home}); err != nil {
		t.Fatal(err)
	}
	if _, ok, err := session.ConnectionTarget(context.Background()); ok || !errors.Is(err, ErrNotOpen) {
		t.Fatalf("closed lookup: ok=%v err=%v", ok, err)
	}
}
