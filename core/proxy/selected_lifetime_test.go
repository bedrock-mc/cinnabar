package proxy

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"log/slog"
	"net"
	"strings"
	"sync/atomic"
	"testing"
	"testing/synctest"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestSelectedTransportHealthySelectionStaysReadyUntilJoin(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		transport := new(countedTransport)
		var attempts atomic.Int32
		selected := selectionFixture(t.Context(), 0, func(context.Context, string) (net.Conn, error) {
			attempts.Add(1)
			return transport, nil
		})
		defer selected.close()
		selected.set("server:1")
		synctest.Wait()
		time.Sleep(time.Minute)
		synctest.Wait()
		prepared := selected.claim("server:1")
		if prepared == nil || attempts.Load() != 1 || transport.closes.Load() != 0 {
			t.Fatal("healthy selection expired or redialed while the server remained selected")
		}
		if _, err := prepared.DialContext(t.Context(), "server:1"); err != nil {
			t.Fatal(err)
		}
		prepared.finish(true)
		time.Sleep(time.Minute)
		synctest.Wait()
		if attempts.Load() != 1 || transport.closes.Load() != 0 {
			t.Fatal("claimed selection was renewed or its retained socket was closed")
		}
	})
}

func TestSelectedTransportPeerClosureRenewsOnlyOneAttempt(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		peer, closePeer := context.WithCancel(t.Context())
		first := &selectedPeerFixture{countedTransport: new(countedTransport), ctx: peer}
		second := new(countedTransport)
		var attempts atomic.Int32
		selected := selectionFixture(t.Context(), 0, func(context.Context, string) (net.Conn, error) {
			if attempts.Add(1) == 1 {
				return first, nil
			}
			if first.closes.Load() != 1 {
				t.Error("renewal overlapped the abandoned socket")
			}
			return second, nil
		})
		defer selected.close()
		selected.set("server:1")
		synctest.Wait()
		closePeer()
		time.Sleep(3 * time.Second)
		synctest.Wait()
		prepared := selected.claim("server:1")
		if prepared == nil || attempts.Load() != 2 {
			t.Fatal("peer closure did not renew the still-selected target")
		}
		prepared.finish(false)
	})
}

func TestSelectedTransportRetryBackoffStopsWhenSelectionClears(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		var attempts atomic.Int32
		selected := selectionFixture(t.Context(), 0, func(context.Context, string) (net.Conn, error) {
			attempts.Add(1)
			return nil, errors.New("transient setup failure")
		})
		defer selected.close()
		selected.set("server:1")
		synctest.Wait()
		time.Sleep(30 * time.Second)
		synctest.Wait()
		if attempts.Load() < 2 || attempts.Load() > 6 {
			t.Fatalf("preparation did not retry with backoff: %d attempts", attempts.Load())
		}
		selected.set("")
		synctest.Wait()
		stopped := attempts.Load()
		time.Sleep(2 * time.Minute)
		synctest.Wait()
		if attempts.Load() != stopped {
			t.Fatal("cleared selection kept retrying")
		}
	})
}

func TestSelectedTransportUnfinishedAttemptIsBoundedAndJoinedBeforeRetry(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		var started, stopped atomic.Int32
		selected := selectionFixture(t.Context(), 0, func(ctx context.Context, _ string) (net.Conn, error) {
			if started.Add(1)-stopped.Load() != 1 {
				t.Error("renewal started before canceled setup finished")
			}
			<-ctx.Done()
			stopped.Add(1)
			return nil, ctx.Err()
		})
		selected.set("server:1")
		synctest.Wait()
		time.Sleep(25 * time.Second)
		synctest.Wait()
		selected.close()
		if started.Load() < 2 || started.Load() > 3 || stopped.Load() != started.Load() {
			t.Fatal("unfinished preparation was not bounded, retried and joined")
		}
	})
}

func TestSelectedTransportServerRejectionDoesNotRetryUntilSelectionChanges(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		var attempts atomic.Int32
		selected := selectionFixture(t.Context(), 0, func(context.Context, string) (net.Conn, error) {
			attempts.Add(1)
			return nil, &minecraft.DisconnectPacketError{}
		})
		defer selected.close()
		selected.set("server:1")
		synctest.Wait()
		time.Sleep(time.Minute)
		synctest.Wait()
		selected.set("server:1")
		synctest.Wait()
		if attempts.Load() != 1 {
			t.Fatal("explicit server rejection was retried")
		}
		selected.set("other:1")
		synctest.Wait()
		if attempts.Load() != 2 {
			t.Fatal("rejected preparation starved a changed selection")
		}
	})
}

func TestSelectedTransportPeerTimeoutRenewsWithFreshAttempt(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		var attempts atomic.Int32
		transport := new(countedTransport)
		selected := selectionFixture(t.Context(), 0, func(context.Context, string) (net.Conn, error) {
			if attempts.Add(1) == 1 {
				return nil, &minecraft.DisconnectPacketError{Reason: packet.DisconnectReasonTimeout}
			}
			return transport, nil
		})
		defer selected.close()
		selected.set("server:1")
		synctest.Wait()
		time.Sleep(2 * time.Second)
		synctest.Wait()
		prepared := selected.claim("server:1")
		if prepared == nil || attempts.Load() != 2 {
			t.Fatal("explicit pre-login timeout did not prepare a fresh attempt")
		}
		prepared.finish(false)
	})
}

func TestSelectedTransportRepeatedShortConnectionsKeepBackingOff(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		var attempts atomic.Int32
		selected := selectionFixture(t.Context(), 0, func(context.Context, string) (net.Conn, error) {
			attempts.Add(1)
			peer, closePeer := context.WithCancel(t.Context())
			go func() {
				time.Sleep(time.Second)
				closePeer()
			}()
			return &selectedPeerFixture{countedTransport: new(countedTransport), ctx: peer}, nil
		})
		defer selected.close()
		selected.set("server:1")
		synctest.Wait()
		time.Sleep(30 * time.Second)
		synctest.Wait()
		if attempts.Load() < 2 || attempts.Load() > 5 {
			t.Fatal("short-lived peer connections reset retry backoff")
		}
	})
}

func TestSelectedTransportLifecycleDiagnosticsContainOnlyBoundedMetadata(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		var output bytes.Buffer
		selected := selectionFixture(t.Context(), 0, func(context.Context, string) (net.Conn, error) {
			return nil, errors.New("secret credential failure")
		})
		selected.logger = slog.New(slog.NewJSONHandler(&output, nil))
		selected.set("secret-target:1")
		synctest.Wait()
		selected.set("")
		synctest.Wait()
		selected.close()
		if strings.Contains(output.String(), "secret") {
			t.Fatal("preparation telemetry exposed a target or failure payload")
		}
		var started, failed bool
		for _, line := range bytes.Split(bytes.TrimSpace(output.Bytes()), []byte{'\n'}) {
			var row map[string]any
			if err := json.Unmarshal(line, &row); err != nil {
				t.Fatal(err)
			}
			if len(row) != 6 || row["msg"] != "JOIN_PREPARATION" || row["preparation_id"].(float64) <= 0 {
				t.Fatal("preparation diagnostics included unbounded metadata")
			}
			started = started || row["status"] == "started"
			failed = failed || row["status"] == "failed"
		}
		if !started || !failed {
			t.Fatal("failed preparation did not report its bounded lifecycle")
		}
	})
}

type selectedPeerFixture struct {
	*countedTransport
	ctx context.Context
}

func (p *selectedPeerFixture) Context() context.Context { return p.ctx }
