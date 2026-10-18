package proxy

import (
	"io"
	"log/slog"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/joinprofile"
	"github.com/sandertv/gophertunnel/minecraft"
)

func TestJoinPackProfileDebugDisabledPreservesDialer(t *testing.T) {
	backend := &scriptedResourcePackCache{}
	calls := 0
	options := &slog.HandlerOptions{}
	if !joinprofile.Enabled {
		options.Level = slog.LevelDebug
	}
	dialer, finish := withJoinPackProfile(t.Context(), minecraft.Dialer{
		ResourcePackCache:    backend,
		ResourcePackProgress: func(minecraft.ResourcePackEvent) { calls++ },
	}, slog.New(slog.NewTextHandler(io.Discard, options)))
	finish()
	dialer.ResourcePackProgress(minecraft.ResourcePackEvent{})
	if calls != 1 || dialer.ResourcePackCache != backend || dialer.PacketObserver.Func != nil || len(dialer.PacketObserver.PacketIDs) != 0 {
		t.Fatal("disabled pack diagnostics changed callbacks, cache or packet observation")
	}
}
