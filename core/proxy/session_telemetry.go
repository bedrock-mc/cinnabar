package proxy

import (
	"context"
	"errors"
	"fmt"
	"io"
	"math"
	"net"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
)

// reportSessionTerminal records the first relay failure before teardown changes either leg.
func reportSessionTerminal(ctx context.Context, direction string, err error, downstream, upstream packetSession) {
	telemetry, ok := ctx.Value(joinTelemetryKey{}).(*joinTelemetry)
	if !ok || telemetry == nil || telemetry.logger == nil {
		return
	}
	_ = callSafely("reporting session terminal", func() error {
		attrs := []any{
			"attempt_id", telemetry.attempt,
			"elapsed_ms", time.Since(telemetry.started).Seconds() * 1000,
			"direction", direction,
			"cause", sessionTerminalCause(err),
			"error_type", fmt.Sprintf("%T", err),
			"relay_context", sessionContextState(ctx),
			"downstream_context", packetSessionContextState(downstream),
			"upstream_context", packetSessionContextState(upstream),
		}
		var disconnect *minecraft.DisconnectPacketError
		if errors.As(err, &disconnect) && disconnect != nil {
			attrs = append(attrs, "disconnect_reason", disconnect.Reason)
		}
		var receive interface{ ReceiveStage() string }
		if errors.As(err, &receive) {
			stage := receive.ReceiveStage()
			switch stage {
			case "decoder", "packet", "callback":
			default:
				stage = "unknown"
			}
			attrs = append(attrs, "receive_stage", stage)
			var packet interface{ PacketID() uint32 }
			if errors.As(err, &packet) && packet.PacketID() > 0 {
				attrs = append(attrs, "protocol_packet_id", packet.PacketID())
			}
		}
		attrs = appendTransportTerminal(attrs, err)
		telemetry.logger.Info("SESSION_TERMINAL", attrs...)
		return nil
	})
}

func appendTransportTerminal(attrs []any, err error) []any {
	var transport interface{ TransportCloseReason() string }
	if !errors.As(err, &transport) {
		return attrs
	}
	reason := transport.TransportCloseReason()
	switch reason {
	case "remote_disconnect", "inactivity_timeout", "local_close", "listener_closed", "dial_cancelled", "raw_read_closed", "raw_read_deadline":
	default:
		reason = "unknown"
	}
	attrs = append(attrs, "transport_close_reason", reason)
	var timings interface {
		TransportIdleMilliseconds() float64
		TransportRTTMilliseconds() float64
		TransportCloseDelayMilliseconds() float64
	}
	if errors.As(err, &timings) {
		for _, value := range []struct {
			name string
			ms   float64
		}{
			{"transport_idle_ms", timings.TransportIdleMilliseconds()},
			{"transport_rtt_ms", timings.TransportRTTMilliseconds()},
			{"transport_close_delay_ms", timings.TransportCloseDelayMilliseconds()},
		} {
			if value.ms >= 0 && !math.IsNaN(value.ms) && !math.IsInf(value.ms, 0) {
				attrs = append(attrs, value.name, value.ms)
			}
		}
	}
	return attrs
}

func sessionTerminalCause(err error) string {
	var disconnect *minecraft.DisconnectPacketError
	var network net.Error
	switch {
	case err == nil:
		return "completed"
	case errors.As(err, &disconnect):
		return "server_disconnect"
	case errors.Is(err, context.Canceled):
		return "cancelled"
	case errors.Is(err, context.DeadlineExceeded):
		return "deadline"
	case errors.Is(err, io.ErrUnexpectedEOF):
		return "truncated"
	case errors.Is(err, io.EOF):
		return "eof"
	case errors.As(err, &network) && network.Timeout():
		return "timeout"
	case streamnet.IsClosed(err):
		return "closed"
	default:
		return "error"
	}
}

func sessionContextState(ctx context.Context) string {
	if ctx.Err() == nil {
		return "active"
	}
	return sessionTerminalCause(ctx.Err())
}

func packetSessionContextState(session packetSession) string {
	switch session := session.(type) {
	case *transferObservingSession:
		return packetSessionContextState(session.upstreamSession)
	case *disconnectObservingSession:
		return packetSessionContextState(session.upstreamSession)
	case interface{ Context() context.Context }:
		return sessionContextState(session.Context())
	default:
		return "unavailable"
	}
}
