package proxy

import (
	"context"
	"errors"
	"log/slog"
	"net"
	"sync"
	"sync/atomic"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/sandertv/gophertunnel/minecraft"
)

type authenticationWarmup struct {
	cancel context.CancelFunc
	done   <-chan struct{}
	failed atomic.Bool
}

func (warmup *authenticationWarmup) stop() {
	warmup.cancel()
	<-warmup.done
}

func warmUpstreamAuthentication(ctx context.Context, account *authcache.Account, logger *slog.Logger) *authenticationWarmup {
	if account == nil {
		done := make(chan struct{})
		close(done)
		return &authenticationWarmup{cancel: func() {}, done: done}
	}
	return startAuthenticationWarmup(ctx, logger, func(ctx context.Context) error {
		connection, err := (minecraft.Dialer{TokenSource: account, ErrorLog: slog.New(slog.DiscardHandler)}).DialContextNetwork(ctx, authenticationOnlyNetwork{}, "")
		if connection != nil {
			_ = connection.Abort()
			_ = connection.Close()
		}
		if errors.Is(err, errAuthenticationPrepared) {
			return nil
		}
		if err == nil {
			return errors.New("proxy: authentication warmup did not reach its transport boundary")
		}
		return err
	}, func(ctx context.Context) error {
		_, err := account.ServiceToken(ctx)
		return err
	})
}

// Warm verifier keys and the account credential independently while the launcher remains usable.
func startAuthenticationWarmup(ctx context.Context, logger *slog.Logger, identity, credential func(context.Context) error) *authenticationWarmup {
	ctx, cancel := context.WithTimeout(ctx, 15*time.Second)
	done := make(chan struct{})
	warmup := &authenticationWarmup{cancel: cancel, done: done}
	var workers sync.WaitGroup
	for name, warm := range map[string]func(context.Context) error{"identity": identity, "credential": credential} {
		workers.Go(func() {
			started := time.Now()
			err := callSafely("warming upstream authentication", func() error { return warm(ctx) })
			if err != nil {
				warmup.failed.Store(true)
			}
			_ = callSafely("reporting authentication warmup", func() error {
				logger.Info("JOIN_AUTH_WARMUP", "stage", name, "duration_ms", time.Since(started).Seconds()*1000, "success", err == nil)
				return nil
			})
		})
	}
	go func() {
		defer close(done)
		workers.Wait()
		cancel()
	}()
	return warmup
}

var errAuthenticationPrepared = errors.New("proxy: authentication prepared without opening a transport")

// The dialer verifies a fresh key-bound token before asking this network for a connection.
// Nothing is sent to a game server; the discarded token warms the shared verifier and account.
type authenticationOnlyNetwork struct{}

func (authenticationOnlyNetwork) DialContext(ctx context.Context, _ string) (net.Conn, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return nil, errAuthenticationPrepared
}

func (authenticationOnlyNetwork) PingContext(context.Context, string) ([]byte, error) {
	return nil, errAuthenticationPrepared
}

func (authenticationOnlyNetwork) Listen(string) (minecraft.NetworkListener, error) {
	return nil, errAuthenticationPrepared
}
