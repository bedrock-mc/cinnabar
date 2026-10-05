package proxy

import (
	"context"
	"net"
	"sync"
	"sync/atomic"
	"testing"
	"testing/synctest"
	"time"
)

type latePreparationTransport struct {
	countedTransport
	lifetime context.Context
}

func (transport *latePreparationTransport) Context() context.Context { return transport.lifetime }

func TestSelectedTransportClaimBeforePreparationAttachesStopsSelection(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		lifetime, cancelLifetime := context.WithCancel(t.Context())
		defer cancelLifetime()
		transport := &latePreparationTransport{lifetime: lifetime}
		entered, release := make(chan struct{}), make(chan struct{})
		var releaseOnce sync.Once
		releasePreparation := func() { releaseOnce.Do(func() { close(release) }) }
		var attempts atomic.Int32
		selected := newSelectedTransport(t.Context(), 0, func(ctx context.Context, target string) *preparedTransport {
			prepared := newPreparedTransport(ctx, transportFixture{dial: func(context.Context, string) (net.Conn, error) {
				return transport, nil
			}}, target)
			<-prepared.done
			if attempts.Add(1) == 1 {
				close(entered)
				<-release
			}
			return prepared
		})
		defer selected.close()
		defer releasePreparation()
		selected.set("server:1")
		<-entered
		if selected.claim("server:1") != nil {
			t.Fatal("Join claimed a preparation the factory had not attached")
		}
		releasePreparation()
		synctest.Wait()
		selected.mu.Lock()
		retained := selected.wanted != "" || selected.pending != nil
		selected.mu.Unlock()
		if retained || transport.closes.Load() != 1 {
			t.Error("Join retained the late preparation instead of disposing it once")
		}
		cancelLifetime()
		time.Sleep(selectedHealthInterval + selectedRetryMax*2)
		synctest.Wait()
		if attempts.Load() != 1 || transport.closes.Load() != 1 {
			t.Error("Join allowed the abandoned selection to renew")
		}
	})
}
