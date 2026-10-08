package proxy

import (
	"bytes"
	"context"
	"log/slog"
	"strings"
	"testing"
)

func TestVerifierPreloadStopCancelsAndJoins(t *testing.T) {
	started, finished := make(chan struct{}), make(chan struct{}, 1)
	stop := startVerifierPreload(t.Context(), slog.New(slog.DiscardHandler), func(ctx context.Context) error {
		close(started)
		<-ctx.Done()
		finished <- struct{}{}
		return ctx.Err()
	})
	<-started
	stop()
	if len(finished) != 1 {
		t.Fatal("stop returned before the preload finished")
	}
}

func TestVerifierPreloadContainsAndRedactsFailures(t *testing.T) {
	var output bytes.Buffer
	startVerifierPreload(t.Context(), slog.New(slog.NewJSONHandler(&output, nil)), func(context.Context) error {
		panic("private authentication panic")
	})()
	if !strings.Contains(output.String(), `"success":false`) || strings.Contains(output.String(), "private") {
		t.Fatalf("preload failure was lost or exposed details: %s", output.String())
	}
}
