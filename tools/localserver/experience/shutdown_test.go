package experience

import (
	"context"
	"errors"
	"io"
	"sync"
	"testing"
	"time"

	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
)

// TestLeaveCommitsAfterWorldClose retains final scoped writes without any live world authority.
func TestLeaveCommitsAfterWorldClose(t *testing.T) {
	f := newRuntimeFixture(t)
	f.do(f.source, func(tx *world.Tx) {
		e, _ := f.actor.Entity(tx)
		f.h.PlayerDisconnected(e.(*player.Player))
		tx.RemoveEntity(e)
		_ = f.actor.Close()
	})
	<-f.d.events
	ev := <-f.d.events
	snap, err := f.h.snapshot(t.Context(), f.d, &ev)
	if err != nil {
		t.Fatal(err)
	}
	if err := f.source.Close(); err != nil {
		t.Fatal(err)
	}
	data := "01"
	f.h.commit(t.Context(), f.d, ev, snap, []Op{{StoreWrite: &StoreWriteOp{Key: StoreKey{Scope: StorePlayer, Schema: 1}, Data: &data}}})
	state := f.h.store.readStates("probe", []stateAddress{{Scope: StorePlayer, Owner: f.actor.UUID().String()}})[0]
	if state.Data == nil || *state.Data != data {
		t.Fatal("world closure lost final private write")
	}
}

// TestDrainPersistsQueuedAndRunningLeaves covers paused queues and a result already in flight.
func TestDrainPersistsQueuedAndRunningLeaves(t *testing.T) {
	for _, mode := range []string{"queued", "running"} {
		t.Run(mode, func(t *testing.T) {
			f := newRuntimeFixture(t)
			sup, _ := startFake(t, "leave_write", f.h.log, startOptions{})
			sup.registered.Worlds = f.d.sup.registered.Worlds
			f.d.sup = sup
			<-f.d.events
			f.do(f.source, func(tx *world.Tx) {
				e, _ := f.actor.Entity(tx)
				f.h.PlayerDisconnected(e.(*player.Player))
			})
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			if mode == "queued" {
				f.h.Pause(true)
				if err := f.h.Drain(ctx); err != nil {
					t.Fatal(err)
				}
			} else {
				entered, release, done := make(chan struct{}), make(chan struct{}), make(chan struct{})
				f.h.afterCall = func() { close(entered); <-release }
				go func() { defer close(done); f.h.Run(context.Background()) }()
				<-entered
				if err := f.source.Close(); err != nil {
					t.Fatal(err)
				}
				drained := make(chan error, 1)
				go func() { drained <- f.h.Drain(ctx) }()
				select {
				case err := <-drained:
					t.Fatalf("drain returned before the running commit: %v", err)
				default:
				}
				close(release)
				if err := <-drained; err != nil {
					t.Fatal(err)
				}
				<-done
			}
			reopened := openTestStore(t, f.h.store.dir)
			state := reopened.readStates("probe", []stateAddress{{Scope: StorePlayer, Owner: f.actor.UUID().String()}})[0]
			if state.Revision != 1 || state.Data == nil || *state.Data != "01" {
				t.Fatalf("final write not flushed: %+v", state)
			}
		})
	}
}

// TestDrainReportsDeadline never reports a canceled drain as complete.
func TestDrainReportsDeadline(t *testing.T) {
	f := newRuntimeFixture(t)
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if err := f.h.Drain(ctx); !errors.Is(err, context.Canceled) {
		t.Fatalf("drain cancellation = %v", err)
	}
}

// callbackWriter announces the first callback write before forwarding it to the helper.
type callbackWriter struct {
	io.WriteCloser
	started chan struct{}
	once    sync.Once
}

// Write marks a callback already in flight without a timer or polling loop.
func (w *callbackWriter) Write(data []byte) (int, error) {
	w.once.Do(func() { close(w.started) })
	return w.WriteCloser.Write(data)
}

// TestDrainCancelsRunningHelper stops a stuck callback without starting helper recovery.
func TestDrainCancelsRunningHelper(t *testing.T) {
	f := newRuntimeFixture(t)
	sup, _ := startFake(t, "hang", f.h.log, startOptions{})
	sup.registered.Worlds = f.d.sup.registered.Worlds
	f.d.sup = sup
	started := make(chan struct{})
	sup.proc.stdin = &callbackWriter{WriteCloser: sup.proc.stdin, started: started}
	restarts := 0
	sup.opts.spawned = func(*helper) { restarts++ }
	done := make(chan struct{})
	go func() { defer close(done); f.h.Run(context.Background()) }()
	<-started
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if err := f.h.Drain(ctx); !errors.Is(err, context.Canceled) {
		t.Fatalf("drain = %v", err)
	}
	<-done
	if restarts != 0 {
		t.Fatal("canceled shutdown restarted its helper")
	}
}

// TestClosedAdmissionRejectsUntrackedActorWork fences callbacks captured before session admission.
func TestClosedAdmissionRejectsUntrackedActorWork(t *testing.T) {
	f := newRuntimeFixture(t)
	f.do(f.source, func(tx *world.Tx) { e, _ := f.actor.Entity(tx); f.h.PlayerDisconnected(e.(*player.Player)) })
	f.h.stopAdmission()
	if f.h.playerCurrent(event{actor: f.actor}) {
		t.Fatal("closed admission retained untracked actor authority")
	}
}
