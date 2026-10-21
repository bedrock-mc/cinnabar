//go:build joinprofile

// Package joinprofile records diagnostic join timings only in tagged developer builds.
package joinprofile

import (
	"context"
	"log/slog"
	"time"
)

// Enabled selects timing instrumentation in developer builds.
const Enabled = true

type contextKey struct{}
type attempt struct {
	id      uint64
	started time.Time
}

// Start associates the local client's correlation ID with this session alone.
func Start(ctx context.Context, id uint64) context.Context {
	if id == 0 {
		return ctx
	}
	ctx = context.WithValue(ctx, contextKey{}, attempt{id, time.Now()})
	Mark(ctx, "core_connect_received")
	return ctx
}

// Mark records one milestone relative to the core receiving the request.
func Mark(ctx context.Context, stage string) {
	if p, ok := ctx.Value(contextKey{}).(attempt); ok {
		slog.Debug("join milestone", "join_profile_id", p.id, "stage", stage, "core_elapsed_ms", float64(time.Since(p.started).Microseconds())/1000)
	}
}

// Stage returns a completion callback for a section that may overlap other sections.
func Stage(ctx context.Context, stage string) func() {
	p, ok := ctx.Value(contextKey{}).(attempt)
	if !ok {
		return func() {}
	}
	started := time.Now()
	Mark(ctx, stage)
	return func() {
		slog.Debug("join section ended", "join_profile_id", p.id, "stage", stage, "duration_ms", float64(time.Since(started).Microseconds())/1000, "core_elapsed_ms", float64(time.Since(p.started).Microseconds())/1000)
	}
}

// Logger attaches a session's correlation ID without changing a shared logger.
func Logger(ctx context.Context, logger *slog.Logger) *slog.Logger {
	if p, ok := ctx.Value(contextKey{}).(attempt); ok {
		return logger.With("join_profile_id", p.id)
	}
	return logger
}
