package lifeline

import (
	"bytes"
	"context"
	"io"
	"log/slog"
	"os"
	"strings"
	"syscall"
	"testing"
	"time"
)

type exitRecorder chan int

func (r exitRecorder) exit(code int) { r <- code }

func start(t *testing.T, cfg Config) (context.Context, func(), exitRecorder) {
	t.Helper()
	exits := make(exitRecorder, 1)
	if cfg.Signals == nil {
		cfg.Signals = make(chan os.Signal)
	}
	cfg.Stderr, cfg.Exit = io.Discard, exits.exit
	if cfg.Log == nil {
		cfg.Log = slog.New(slog.DiscardHandler)
	}
	ctx, stop := Start(context.Background(), cfg)
	t.Cleanup(stop)
	return ctx, stop, exits
}

func waitDone(t *testing.T, ctx context.Context) {
	t.Helper()
	select {
	case <-ctx.Done():
	case <-time.After(2 * time.Second):
		t.Fatal("context was not cancelled")
	}
}

// A shutdown that ignores its context still ends once the grace period runs out.
func TestGraceBoundsAWedgedShutdown(t *testing.T) {
	signals := make(chan os.Signal, 1)
	ctx, _, exits := start(t, Config{Signals: signals, Grace: 50 * time.Millisecond})
	signals <- syscall.SIGTERM
	waitDone(t, ctx)
	select {
	case code := <-exits:
		if code != 1 {
			t.Fatalf("exit code = %d, want 1", code)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("wedged shutdown never hard-exited")
	}
}

// Why shutdown began and why the process hard-exits reach the structured logger.
func TestShutdownReasonsAreLogged(t *testing.T) {
	var logs bytes.Buffer
	signals := make(chan os.Signal, 1)
	ctx, _, exits := start(t, Config{Signals: signals, Grace: 50 * time.Millisecond, Log: slog.New(slog.NewTextHandler(&logs, nil))})
	signals <- syscall.SIGTERM
	waitDone(t, ctx)
	select {
	case <-exits:
	case <-time.After(2 * time.Second):
		t.Fatal("wedged shutdown never hard-exited")
	}
	output := logs.String()
	for _, want := range []string{
		`level=INFO msg="core shutting down" reason="signal terminated"`,
		`level=ERROR msg="core exiting" reason="shutdown exceeded its grace period"`,
	} {
		if !strings.Contains(output, want) {
			t.Fatalf("log lacks %q:\n%s", want, output)
		}
	}
}

// A signal after cancellation (the client escalating) exits without waiting out the grace.
func TestSecondSignalExitsAtOnce(t *testing.T) {
	signals := make(chan os.Signal, 2)
	ctx, _, exits := start(t, Config{Signals: signals, Grace: time.Hour})
	signals <- syscall.SIGINT
	waitDone(t, ctx)
	signals <- syscall.SIGTERM
	select {
	case <-exits:
	case <-time.After(2 * time.Second):
		t.Fatal("second signal did not exit")
	}
}

func TestStdinEOFAndParentDeathCancel(t *testing.T) {
	ctx, _, _ := start(t, Config{Stdin: strings.NewReader(""), Grace: time.Hour})
	waitDone(t, ctx)
	gone := make(chan struct{})
	ctx, _, _ = start(t, Config{ParentGone: gone, Grace: time.Hour})
	close(gone)
	waitDone(t, ctx)
}

// Finishing shutdown in time disarms the hard exit.
func TestStopDisarmsTheHardExit(t *testing.T) {
	gone := make(chan struct{})
	ctx, stop, exits := start(t, Config{ParentGone: gone, Grace: 50 * time.Millisecond})
	close(gone)
	waitDone(t, ctx)
	stop()
	select {
	case <-exits:
		t.Fatal("hard exit fired after stop")
	case <-time.After(200 * time.Millisecond):
	}
}
