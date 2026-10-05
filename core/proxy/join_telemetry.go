package proxy

import (
	"context"
	"crypto/ecdsa"
	"log/slog"
	"net"
	"sync/atomic"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"golang.org/x/oauth2"
)

type joinTelemetry struct {
	attempt  uint64
	started  time.Time
	logger   *slog.Logger
	seen     [7]atomic.Bool
	complete atomic.Bool
	transfer atomic.Bool
}

type joinTelemetryKey struct{}

func (telemetry *joinTelemetry) report(phase string, duration time.Duration, success bool) {
	_ = callSafely("reporting join timing", func() error {
		telemetry.logger.Info("JOIN_PHASE", "attempt_id", telemetry.attempt, "phase", phase,
			"elapsed_ms", time.Since(telemetry.started).Seconds()*1000,
			"duration_ms", duration.Seconds()*1000, "success", success)
		return nil
	})
}

func reportJoinDuration(ctx context.Context, phase string, started time.Time, success bool) {
	if telemetry, ok := ctx.Value(joinTelemetryKey{}).(*joinTelemetry); ok {
		telemetry.report(phase, time.Since(started), success)
	}
}

type measuredMultiplayerTokenSource struct {
	oauth2.TokenSource
	multiplayer minecraft.MultiplayerTokenSource
	telemetry   *joinTelemetry
}

func (source measuredMultiplayerTokenSource) MultiplayerToken(ctx context.Context, key *ecdsa.PublicKey) (string, error) {
	started := time.Now()
	token, err := source.multiplayer.MultiplayerToken(ctx, key)
	source.telemetry.report("multiplayer_token", time.Since(started), err == nil)
	return token, err
}

func withJoinTelemetry(dialer minecraft.Dialer, telemetry *joinTelemetry) minecraft.Dialer {
	if source, ok := dialer.TokenSource.(minecraft.MultiplayerTokenSource); ok {
		dialer.TokenSource = measuredMultiplayerTokenSource{TokenSource: dialer.TokenSource, multiplayer: source, telemetry: telemetry}
	}
	accept := dialer.AcceptPacketHeader
	dialer.AcceptPacketHeader = func(header packet.Header) bool {
		if header.PacketID == packet.IDTransfer && telemetry.transfer.CompareAndSwap(false, true) {
			telemetry.report("transfer_ingress", 0, true)
		}
		return accept == nil || accept(header)
	}
	observe := dialer.PacketFunc
	dialer.PacketFunc = func(header packet.Header, payload []byte, source, destination net.Addr) {
		if observe != nil {
			observe(header, payload, source, destination)
		}
		if telemetry.complete.Load() {
			return
		}
		phase, index := joinPacketPhase(header.PacketID)
		if index >= 0 && telemetry.seen[index].CompareAndSwap(false, true) {
			telemetry.report(phase, 0, true)
		}
		if header.PacketID == packet.IDStartGame {
			telemetry.complete.Store(true)
		}
	}
	return dialer
}

func joinPacketPhase(id uint32) (string, int) {
	switch id {
	case packet.IDRequestNetworkSettings:
		return "request_network_settings", 0
	case packet.IDNetworkSettings:
		return "network_settings", 1
	case packet.IDLogin:
		return "login", 2
	case packet.IDServerToClientHandshake:
		return "server_handshake", 3
	case packet.IDResourcePacksInfo:
		return "resource_packs_info", 4
	case packet.IDResourcePackStack:
		return "resource_pack_stack", 5
	case packet.IDStartGame:
		return "start_game", 6
	default:
		return "", -1
	}
}
