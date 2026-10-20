//go:build !joinprofile

package proxy

import (
	"github.com/sandertv/gophertunnel/minecraft"
	"log/slog"
)

// withDiagnosticJoinStages leaves shipping packet observers unchanged.
func withDiagnosticJoinStages(d minecraft.Dialer, _ *slog.Logger) minecraft.Dialer { return d }
