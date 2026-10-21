//go:build !joinprofile

// Package joinprofile records diagnostic join timings only in tagged developer builds.
package joinprofile

import (
	"context"
	"log/slog"
	"net/http"
)

// Enabled is false in shipping builds, allowing diagnostic branches to be removed.
const Enabled = false

// Start keeps the original context in shipping builds.
func Start(ctx context.Context, _ uint64) context.Context { return ctx }

// Mark does nothing in shipping builds.
func Mark(context.Context, string) {}

// Stage returns an inert completion callback in shipping builds.
func Stage(context.Context, string) func() { return func() {} }

// Logger preserves the caller's logger in shipping builds.
func Logger(_ context.Context, logger *slog.Logger) *slog.Logger { return logger }

// HTTPClient preserves the caller's client in shipping builds.
func HTTPClient(_ context.Context, client *http.Client) *http.Client { return client }
