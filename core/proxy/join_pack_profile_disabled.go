//go:build !joinprofile

package proxy

import (
	"context"
	"log/slog"

	"github.com/sandertv/gophertunnel/minecraft"
)

// withJoinPackProfile leaves shipping dialers and their callbacks unchanged.
func withJoinPackProfile(_ context.Context, dialer minecraft.Dialer, _ *slog.Logger) (minecraft.Dialer, func()) {
	return dialer, func() {}
}
