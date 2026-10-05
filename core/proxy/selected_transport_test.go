package proxy

import (
	"context"
	"errors"
	"log/slog"
	"net"
	"sync/atomic"
	"testing"
	"testing/synctest"
	"time"

	"github.com/sandertv/go-raknet"
	"github.com/sandertv/gophertunnel/minecraft"
)

func selectionFixture(ctx context.Context, ttl time.Duration, dial func(context.Context, string) (net.Conn, error)) *selectedTransport {
	return newSelectedTransport(ctx, ttl, func(ctx context.Context, target string) *preparedTransport {
		return newPreparedTransport(ctx, transportFixture{dial: dial}, target)
	})
}

func TestSelectedTransportClaimsOnlyFreshActualTargetOnce(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		transport := new(countedTransport)
		var dials atomic.Int32
		selected := selectionFixture(t.Context(), selectedTransportTTL, func(context.Context, string) (net.Conn, error) {
			dials.Add(1)
			return transport, nil
		})
		selector := &UpstreamSelector{preparation: selected}
		defer selected.close()
		selector.PrepareTransport("server:1")
		synctest.Wait()
		selector.PrepareTransport("server:1")
		if prepared := selector.claimTransport("server:1"); prepared != nil {
			t.Fatal("preparation selected a game route without Connect")
		}
		selector.Set("server:1")
		if prepared := selector.claimTransport("different:1"); prepared != nil {
			t.Fatal("preparation was claimed for a different server")
		}
		prepared := selector.claimTransport("server:1")
		if prepared == nil {
			t.Fatal("fresh matching preparation was not claimed")
		}
		conn, err := prepared.DialContext(t.Context(), "server:1")
		if conn != transport || err != nil {
			t.Fatal("prepared handoff changed its concrete transport")
		}
		prepared.finish(true)
		if selector.claimTransport("server:1") != nil || dials.Load() != 1 || transport.closes.Load() != 0 {
			t.Fatal("selection redialed, handed off twice, or closed the retained connection")
		}
	})
}

func TestSelectedTransportExpiresWithoutRepeatingTheSelection(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		transport := new(countedTransport)
		var dials atomic.Int32
		selected := selectionFixture(t.Context(), selectedTransportTTL, func(context.Context, string) (net.Conn, error) {
			dials.Add(1)
			return transport, nil
		})
		defer selected.close()
		selected.set("server:1")
		synctest.Wait()
		time.Sleep(selectedTransportTTL)
		synctest.Wait()
		selected.set("server:1")
		synctest.Wait()
		if selected.claim("server:1") != nil || transport.closes.Load() != 1 || dials.Load() != 1 {
			t.Fatal("expired selection was retained or automatically redialed")
		}
	})
}

func TestSelectedTransportReplacementAndShutdownJoinCanceledDials(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		var started, stopped atomic.Int32
		selected := selectionFixture(t.Context(), selectedTransportTTL, func(ctx context.Context, _ string) (net.Conn, error) {
			if started.Add(1)-stopped.Load() != 1 {
				t.Error("selection started a second idle transport")
			}
			<-ctx.Done()
			stopped.Add(1)
			return nil, ctx.Err()
		})
		selected.set("first:1")
		synctest.Wait()
		selected.set("second:1")
		synctest.Wait()
		selected.set("third:1")
		selected.set("fourth:1")
		synctest.Wait()
		selected.close()
		if started.Load() != stopped.Load() {
			t.Fatal("shutdown left transport setup running")
		}
		selected.set("late:1")
		synctest.Wait()
		if started.Load() != stopped.Load() {
			t.Fatal("closed selection started another transport")
		}
	})
}

func TestSelectedTransportChangingActualRouteCancelsPreparation(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		transport := new(countedTransport)
		selected := selectionFixture(t.Context(), selectedTransportTTL, func(context.Context, string) (net.Conn, error) { return transport, nil })
		defer selected.close()
		selector := &UpstreamSelector{preparation: selected}
		selector.PrepareTransport("first:1")
		synctest.Wait()
		selector.Set("second:1")
		synctest.Wait()
		if selected.claim("first:1") != nil || transport.closes.Load() != 1 {
			t.Fatal("changed actual route retained the old preparation")
		}
	})
}

func TestSelectedTransportClaimDoesNotWaitForUnfinishedOrFailedPreparation(t *testing.T) {
	for _, fails := range []bool{false, true} {
		synctest.Test(t, func(t *testing.T) {
			var stopped atomic.Int32
			selected := selectionFixture(t.Context(), selectedTransportTTL, func(ctx context.Context, _ string) (net.Conn, error) {
				if !fails {
					<-ctx.Done()
				}
				stopped.Add(1)
				return nil, errors.New("preparation failed")
			})
			defer selected.close()
			selected.set("server:1")
			synctest.Wait()
			if selected.claim("server:1") != nil {
				t.Fatal("Join waited on an unfinished or failed preparation")
			}
			synctest.Wait()
			if stopped.Load() != 1 {
				t.Fatal("declined preparation kept negotiating after Join")
			}
		})
	}
}

func TestSelectedTransportClaimCannotBeClosedByItsOldExpiry(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		transport := new(countedTransport)
		selected := selectionFixture(t.Context(), selectedTransportTTL, func(context.Context, string) (net.Conn, error) {
			return transport, nil
		})
		selected.set("server:1")
		synctest.Wait()
		prepared := selected.claim("server:1")
		if prepared == nil {
			t.Fatal("ready preparation was not claimed")
		}
		if _, err := prepared.DialContext(t.Context(), "server:1"); err != nil {
			t.Fatal(err)
		}
		prepared.finish(true)
		time.Sleep(selectedTransportTTL + time.Second)
		synctest.Wait()
		selected.close()
		if transport.closes.Load() != 0 {
			t.Fatal("old expiry or selection shutdown disposed a retained connection")
		}
	})
}

func TestSelectedTransportFailedSelectionDoesNotStarveItsReplacement(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		transport := new(countedTransport)
		var dials atomic.Int32
		selected := selectionFixture(t.Context(), selectedTransportTTL, func(_ context.Context, target string) (net.Conn, error) {
			dials.Add(1)
			if target == "failed:1" {
				return nil, errors.New("preparation failed")
			}
			return transport, nil
		})
		defer selected.close()
		selected.set("failed:1")
		synctest.Wait()
		if selected.claim("failed:1") != nil {
			t.Fatal("failed selection was claimed")
		}
		selected.set("fresh:1")
		synctest.Wait()
		prepared := selected.claim("fresh:1")
		if prepared == nil || dials.Load() != 2 {
			t.Fatal("failed speculation prevented a distinct selection from preparing")
		}
		prepared.finish(false)
	})
}

func TestSelectedTransportFailureFallsBackAndDisposesOnce(t *testing.T) {
	failed := new(countedTransport)
	var disposed atomic.Int32
	prepared := newOwnedPreparedTransport(t.Context(), transportFixture{dial: func(context.Context, string) (net.Conn, error) {
		return failed, errors.New("preparation failed")
	}}, "server:1", func() { disposed.Add(1) })
	successful := new(countedTransport)
	network := fallbackPreparedTransport{prepared: prepared, Network: transportFixture{dial: func(context.Context, string) (net.Conn, error) {
		return successful, nil
	}}}
	conn, err := network.DialContext(t.Context(), "server:1")
	prepared.finish(true)
	if conn != successful || err != nil || failed.closes.Load() != 1 || successful.closes.Load() != 0 || disposed.Load() != 1 {
		t.Fatal("failed preparation broke the normal dial or was disposed twice")
	}
}

type closedPreparedConnection struct {
	countedTransport
	ctx context.Context
}

func (connection *closedPreparedConnection) Context() context.Context { return connection.ctx }

func TestSelectedTransportClosedIdleConnectionFallsBack(t *testing.T) {
	ctx, cancel := context.WithCancel(t.Context())
	closed := &closedPreparedConnection{ctx: ctx}
	prepared := newPreparedTransport(t.Context(), transportFixture{dial: func(context.Context, string) (net.Conn, error) {
		return closed, nil
	}}, "server:1")
	<-prepared.done
	cancel()
	successful := new(countedTransport)
	network := fallbackPreparedTransport{prepared: prepared, Network: transportFixture{dial: func(context.Context, string) (net.Conn, error) {
		return successful, nil
	}}}
	conn, err := network.DialContext(t.Context(), "server:1")
	prepared.finish(true)
	if conn != successful || err != nil || closed.closes.Load() != 1 {
		t.Fatal("closed idle connection prevented an ordinary fresh dial")
	}
}

func TestSelectedTransportDoesNotWrapIdentityNetworks(t *testing.T) {
	fixture := transportFixture{}
	_, err := dialWithSelectedTransport(t.Context(), nil, fixture, "server:1", func(_ context.Context, network minecraft.Network, _ string) (*minecraft.Conn, error) {
		if _, ok := network.(transportFixture); !ok {
			t.Fatal("identity-aware transport was replaced")
		}
		return nil, nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestPreparedTransportDisposesRawSocketOnlyWhenAbandoned(t *testing.T) {
	for _, retain := range []bool{false, true} {
		transport := new(countedTransport)
		var disposed atomic.Int32
		prepared := newOwnedPreparedTransport(t.Context(), transportFixture{dial: func(context.Context, string) (net.Conn, error) {
			return transport, nil
		}}, "server:1", func() { disposed.Add(1) })
		if _, err := prepared.DialContext(t.Context(), "server:1"); err != nil {
			t.Fatal(err)
		}
		prepared.finish(retain)
		prepared.finish(retain)
		want := int32(1)
		if retain {
			want = 0
		}
		if disposed.Load() != want || transport.closes.Load() != want {
			t.Fatal("raw socket disposal did not preserve the successful handoff")
		}
	}
}

func TestSelectedTransportProductionRakNetHasNoLoginAndRetainsItsSocket(t *testing.T) {
	listener, err := (raknet.ListenConfig{ErrorLog: slog.New(slog.DiscardHandler)}).Listen("127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	accepted := make(chan net.Conn, 1)
	acceptDone := make(chan struct{})
	go func() {
		defer close(acceptDone)
		conn, acceptErr := listener.Accept()
		if acceptErr == nil {
			accepted <- conn
		}
	}()
	defer func() { _ = listener.Close(); <-acceptDone }()
	socket := new(transportSocket)
	created := make(chan *preparedTransport, 1)
	selected := newSelectedTransport(t.Context(), selectedTransportTTL, func(ctx context.Context, target string) *preparedTransport {
		prepared := newOwnedPreparedTransport(ctx, minecraft.RakNet{Logger: slog.New(slog.DiscardHandler), UpstreamDialer: socket}, target, socket.close)
		created <- prepared
		return prepared
	})
	defer selected.close()
	address := listener.Addr().String()
	selected.set(address)
	prepared := <-created
	<-prepared.done
	if prepared.err != nil {
		t.Fatal(prepared.err)
	}
	server := <-accepted
	defer server.Close()
	readDone := make(chan struct{})
	application := make(chan int, 1)
	go func() {
		defer close(readDone)
		payload, _ := server.(*raknet.Conn).ReadPacket()
		application <- len(payload)
	}()
	defer func() { _ = listener.Close(); <-readDone }()
	select {
	case bytes := <-application:
		t.Fatalf("idle preparation sent an application packet or closed: bytes=%d", bytes)
	case <-time.After(50 * time.Millisecond):
	}
	claimed := selected.claim(address)
	if claimed != prepared {
		t.Fatal("production selection did not retain the prepared transport")
	}
	conn, err := claimed.DialContext(t.Context(), address)
	if err != nil {
		t.Fatal(err)
	}
	claimed.finish(true)
	if conn.(*raknet.Conn).Context().Err() != nil {
		t.Fatal("successful setup cancellation ended the RakNet connection")
	}
	socket.mu.Lock()
	udp := socket.conn
	socket.mu.Unlock()
	claimed.finish(false)
	if _, err := udp.Write([]byte{0}); !errors.Is(err, net.ErrClosed) {
		t.Fatal("abandoned preparation retained its UDP socket")
	}
}

func BenchmarkSelectedTransportHeadStart(b *testing.B) {
	for _, headStart := range []bool{false, true} {
		b.Run(map[bool]string{false: "join_time_overlap", true: "selection_time_transport"}[headStart], func(b *testing.B) {
			network := transportFixture{dial: func(ctx context.Context, _ string) (net.Conn, error) {
				select {
				case <-time.After(40 * time.Millisecond):
					return new(countedTransport), nil
				case <-ctx.Done():
					return nil, ctx.Err()
				}
			}}
			for b.Loop() {
				b.StopTimer()
				var prepared *preparedTransport
				if headStart {
					prepared = newPreparedTransport(b.Context(), network, "server:1")
					<-prepared.done
				}
				b.StartTimer()
				if prepared == nil {
					prepared = newPreparedTransport(b.Context(), network, "server:1")
				}
				time.Sleep(10 * time.Millisecond)
				if _, err := prepared.DialContext(b.Context(), "server:1"); err != nil {
					b.Fatal(err)
				}
				prepared.finish(false)
			}
		})
	}
}
