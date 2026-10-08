package proxy

import (
	"context"
	"log/slog"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
)

// StartVerifierPreload resolves the account-independent token verifier (discovery, OIDC configuration and
// signing keys) in the background, signed in or not. It sends no account data and fails silently.
func StartVerifierPreload(ctx context.Context, logger *slog.Logger) (stop func()) {
	return startVerifierPreload(ctx, logger, minecraft.PreloadAuthVerifier)
}

func startVerifierPreload(ctx context.Context, logger *slog.Logger, preload func(context.Context) error) (stop func()) {
	ctx, cancel := context.WithCancel(ctx)
	done := make(chan struct{})
	go func() {
		defer close(done)
		started := time.Now()
		err := callSafely("preloading upstream authentication", func() error { return preload(ctx) })
		_ = callSafely("reporting authentication preload", func() error {
			logger.Info("JOIN_AUTH_PRELOAD", "duration_ms", time.Since(started).Seconds()*1000, "success", err == nil)
			return nil
		})
	}()
	return func() {
		cancel()
		<-done
	}
}
