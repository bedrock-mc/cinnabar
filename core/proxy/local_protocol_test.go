package proxy

import (
	"context"
	"errors"
	"log/slog"
	"strings"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
)

// TestServePinsLocalProtocol checks the production listener without a BDS process.
func TestServePinsLocalProtocol(t *testing.T) {
	dir := t.TempDir()
	var output lockedBuffer
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	done := make(chan error, 1)
	go func() {
		done <- Serve(ctx, Config{
			SocketDir: dir,
			Upstream:  "invalid-address-without-port",
			Logger:    slog.New(slog.NewTextHandler(&output, nil)),
		})
	}()
	if !output.waitFor(ctx, "listener ready; waiting for local Rust client") {
		t.Fatalf("listener was not ready: %s", output.String())
	}
	// A future/default client must fail locally without terminating Serve or dialing upstream.
	conn, err := (minecraft.Dialer{
		Protocol:     minecraft.DefaultProtocol,
		IdentityData: login.IdentityData{DisplayName: "Unsupported"},
	}).DialContextNetwork(ctx, streamnet.New(dir), "")
	if conn != nil {
		_ = conn.Close()
	}
	if err == nil || !strings.Contains(output.String(), "unsupported local protocol") {
		t.Fatalf("new default protocol error = %v, want local protocol rejection", err)
	}
	select {
	case err := <-done:
		t.Fatalf("unsupported local protocol stopped Serve: %v", err)
	default:
	}
	// The supported client reaches upstream preparation, whose deliberate address error is observable.
	conn, err = (minecraft.Dialer{
		Protocol:     minecraft.Protocol12644(),
		IdentityData: login.IdentityData{DisplayName: "PinnedProtocol"},
	}).DialContextNetwork(ctx, streamnet.New(dir), "")
	if conn != nil {
		_ = conn.Close()
	}
	if err == nil {
		t.Fatal("invalid upstream unexpectedly connected")
	}
	select {
	case serveErr := <-done:
		if serveErr == nil || errors.Is(serveErr, context.DeadlineExceeded) || !strings.Contains(serveErr.Error(), "missing port") {
			t.Fatalf("supported protocol preparation error = %v, want invalid upstream address", serveErr)
		}
	case <-ctx.Done():
		t.Fatal("supported local protocol did not reach upstream preparation")
	}
}
