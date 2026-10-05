package proxy

import (
	"context"
	"errors"
	"net"
	"sync"
	"sync/atomic"
	"testing"
	"testing/synctest"

	"github.com/sandertv/gophertunnel/minecraft"
)

func TestAdmissionRejectsLatePreparationUntilResourceOwnerCloses(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		var speculativeDials atomic.Int32
		selected := selectionFixture(t.Context(), 0, func(context.Context, string) (net.Conn, error) {
			speculativeDials.Add(1)
			return new(countedTransport), nil
		})
		defer selected.close()
		selector := &UpstreamSelector{preparation: selected, target: "actual.test:1"}
		connections := newTestPreparedConnections()
		connections.selector = selector
		defer connections.shutdown()
		prepared := &preparedConnection{upstream: newFakeUpstream(nil), packStack: &selectedResourcePackStack{}}
		entered, release := make(chan struct{}), make(chan struct{})
		var releaseOnce sync.Once
		releaseSetup := func() { releaseOnce.Do(func() { close(release) }) }
		defer releaseSetup()
		connections.connectPrepared = func(context.Context, dialerDownstream) (*preparedConnection, error) {
			close(entered)
			<-release
			return prepared, nil
		}
		key := new(minecraft.Conn)
		ctx, cancel := context.WithCancel(t.Context())
		defer cancel()
		done := make(chan error, 1)
		go func() { done <- connections.prepareConnection(ctx, key, new(offerTestDownstream)) }()
		<-entered
		selector.PrepareTransport("late-setup.test:1")
		synctest.Wait()
		if speculativeDials.Load() != 0 {
			t.Error("late preparation started another peer during game admission")
		}
		releaseSetup()
		if err := <-done; err != nil {
			t.Fatal(err)
		}
		selector.PrepareTransport("late-offer.test:1")
		synctest.Wait()
		if speculativeDials.Load() != 0 {
			t.Error("retaining the downstream offer ended the admission fence")
		}
		retained, ok := connections.take(key)
		if !ok || retained != prepared {
			t.Fatal("prepared admission did not transfer its exact resource owner")
		}
		selector.PrepareTransport("late-session.test:1")
		synctest.Wait()
		if speculativeDials.Load() != 0 {
			t.Error("handoff ended the admission fence before session close")
		}
		cleanupStarted, cleanupRelease := make(chan struct{}), make(chan struct{})
		var cleanupOnce sync.Once
		finishCleanup := func() { cleanupOnce.Do(func() { close(cleanupRelease) }) }
		defer finishCleanup()
		retained.releaseTarget = func() error {
			close(cleanupStarted)
			<-cleanupRelease
			return nil
		}
		closed := make(chan error, 1)
		go func() { closed <- retained.close() }()
		<-cleanupStarted
		selector.PrepareTransport("late-cleanup.test:1")
		synctest.Wait()
		if speculativeDials.Load() != 0 {
			t.Error("session close released admission before protocol resource cleanup")
		}
		finishCleanup()
		if err := <-closed; err != nil {
			t.Fatal(err)
		}
		selector.PrepareTransport("menu.test:1")
		synctest.Wait()
		if speculativeDials.Load() != 1 {
			t.Error("closed game admission did not allow one fresh menu preparation")
		}
	})
}

func TestAdmissionKeepsMatchingReadyPreparationClaimable(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		transport := new(countedTransport)
		var speculativeDials atomic.Int32
		selected := selectionFixture(t.Context(), 0, func(context.Context, string) (net.Conn, error) {
			speculativeDials.Add(1)
			return transport, nil
		})
		defer selected.close()
		selector := &UpstreamSelector{preparation: selected, target: "selected.test:1"}
		selector.PrepareTransport("selected.test:1")
		synctest.Wait()
		selected.mu.Lock()
		original := selected.pending.prepared
		selected.mu.Unlock()
		release := selector.beginAdmission()
		defer release()
		selector.PrepareTransport("late.test:1")
		selector.PrepareTransport("")
		synctest.Wait()
		claimed := selected.claim("selected.test:1")
		if claimed == nil || claimed != original || speculativeDials.Load() != 1 || transport.closes.Load() != 0 {
			t.Fatal("admission replaced or canceled the matching ready preparation")
		}
		claimed.finish(false)
	})
}

func TestAdmissionAbandonedOfferReopensMenuPreparation(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		var speculativeDials atomic.Int32
		selected := selectionFixture(t.Context(), 0, func(context.Context, string) (net.Conn, error) {
			speculativeDials.Add(1)
			return new(countedTransport), nil
		})
		defer selected.close()
		selector := &UpstreamSelector{preparation: selected}
		connections := newTestPreparedConnections()
		connections.selector = selector
		defer connections.shutdown()
		connections.connectPrepared = func(context.Context, dialerDownstream) (*preparedConnection, error) {
			return &preparedConnection{upstream: newFakeUpstream(nil), packStack: &selectedResourcePackStack{}}, nil
		}
		ctx, cancel := context.WithCancel(t.Context())
		defer cancel()
		key := new(minecraft.Conn)
		if err := connections.prepareConnection(ctx, key, new(offerTestDownstream)); err != nil {
			t.Fatal(err)
		}
		selector.PrepareTransport("late.test:1")
		synctest.Wait()
		if speculativeDials.Load() != 0 {
			t.Fatal("unclaimed downstream offer did not keep its admission fence")
		}
		cancel()
		synctest.Wait()
		if _, ok := connections.take(key); ok {
			t.Fatal("canceled downstream retained its admission resources")
		}
		selector.PrepareTransport("menu.test:1")
		synctest.Wait()
		if speculativeDials.Load() != 1 {
			t.Fatal("abandoned downstream offer stranded the admission fence")
		}
	})
}

func TestAdmissionCancelsUnmatchedPreparation(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		transport := new(countedTransport)
		selected := selectionFixture(t.Context(), 0, func(context.Context, string) (net.Conn, error) {
			return transport, nil
		})
		defer selected.close()
		selector := &UpstreamSelector{preparation: selected, target: "actual.test:1"}
		selector.PrepareTransport("late.test:1")
		synctest.Wait()
		release := selector.beginAdmission()
		defer release()
		synctest.Wait()
		if selected.claim("late.test:1") != nil || transport.closes.Load() != 1 {
			t.Fatal("admission retained a late preparation for another target")
		}
	})
}

func TestAdmissionOverlappingLeasesReleaseOnlyTheirOwnOwnership(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		var speculativeDials atomic.Int32
		selected := selectionFixture(t.Context(), 0, func(context.Context, string) (net.Conn, error) {
			speculativeDials.Add(1)
			return new(countedTransport), nil
		})
		defer selected.close()
		selector := &UpstreamSelector{preparation: selected}
		old := selector.beginAdmission()
		current := selector.beginAdmission()
		defer old()
		defer current()
		var repeated sync.WaitGroup
		for range 8 {
			repeated.Go(old)
		}
		repeated.Wait()
		selector.PrepareTransport("late.test:1")
		synctest.Wait()
		if speculativeDials.Load() != 0 {
			t.Fatal("old transfer admission reopened preparation during the new admission")
		}
		for range 8 {
			repeated.Go(current)
		}
		repeated.Wait()
		selector.PrepareTransport("menu.test:1")
		synctest.Wait()
		if speculativeDials.Load() != 1 {
			t.Fatal("last transfer admission release did not reopen menu preparation once")
		}
		latest := selector.beginAdmission()
		defer latest()
		old()
		current()
		selector.PrepareTransport("another-late.test:1")
		synctest.Wait()
		if speculativeDials.Load() != 1 {
			t.Fatal("repeated stale release ended a later admission")
		}
	})
}

func TestAdmissionFailureReopensMenuPreparation(t *testing.T) {
	for _, failure := range []string{"dial", "cancel", "configure", "panic"} {
		t.Run(failure, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				var speculativeDials atomic.Int32
				selected := selectionFixture(t.Context(), 0, func(context.Context, string) (net.Conn, error) {
					speculativeDials.Add(1)
					return new(countedTransport), nil
				})
				defer selected.close()
				selector := &UpstreamSelector{preparation: selected}
				connections := newTestPreparedConnections()
				connections.selector = selector
				defer connections.shutdown()
				ctx, cancel := context.WithCancel(t.Context())
				defer cancel()
				want := errors.New("fixture admission failure")
				downstream := new(offerTestDownstream)
				connections.connectPrepared = func(ctx context.Context, _ dialerDownstream) (*preparedConnection, error) {
					switch failure {
					case "dial":
						return nil, want
					case "cancel":
						cancel()
						return nil, ctx.Err()
					case "panic":
						panic("fixture admission panic")
					default:
						downstream.err = want
						return &preparedConnection{upstream: newFakeUpstream(nil), packStack: &selectedResourcePackStack{}}, nil
					}
				}
				var err error
				var recovered any
				func() {
					defer func() { recovered = recover() }()
					err = connections.prepareConnection(ctx, new(minecraft.Conn), downstream)
				}()
				if err == nil && recovered == nil {
					t.Fatal("fixture admission unexpectedly succeeded")
				}
				selector.PrepareTransport("menu.test:1")
				synctest.Wait()
				if speculativeDials.Load() != 1 {
					t.Fatal("failed admission stranded the menu preparation fence")
				}
			})
		})
	}
}
