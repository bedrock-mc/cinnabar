package proxy

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"strings"
	"testing"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestRelayReportsOriginalTerminalBeforeCleanup(t *testing.T) {
	for _, test := range []struct {
		name  string
		err   error
		cause string
	}{
		{"eof", fmt.Errorf("private credential: %w", io.EOF), "eof"},
		{"truncation", fmt.Errorf("private credential: %w", io.ErrUnexpectedEOF), "truncated"},
		{"failure", errors.New("private credential"), "error"},
		{"disconnect", &minecraft.DisconnectPacketError{Reason: packet.DisconnectReasonKicked, Message: "private credential"}, "server_disconnect"},
	} {
		t.Run(test.name, func(t *testing.T) {
			var output bytes.Buffer
			telemetry := &joinTelemetry{attempt: 71, started: time.Now(), logger: slog.New(slog.NewJSONHandler(&output, nil))}
			ctx := context.WithValue(t.Context(), joinTelemetryKey{}, telemetry)
			down, up := newFakeSession(), newFakeSession()
			up.reads <- packetResult{err: test.err}
			_ = relayWithSessions(ctx, &down, &up)
			row := readSessionTerminal(t, &output)
			if row["attempt_id"] != float64(71) || row["direction"] != "upstream to downstream" || row["cause"] != test.cause || row["relay_context"] != "active" {
				t.Fatalf("lost original terminal identity: %v", row)
			}
			if test.cause == "server_disconnect" && row["disconnect_reason"] != float64(packet.DisconnectReasonKicked) {
				t.Fatalf("lost server disconnect reason: %v", row)
			}
			if strings.Contains(output.String(), "private credential") {
				t.Fatal("terminal telemetry exposed error or packet contents")
			}
		})
	}
}

func TestRelayReportsContextCancellation(t *testing.T) {
	var output bytes.Buffer
	telemetry := &joinTelemetry{attempt: 19, started: time.Now(), logger: slog.New(slog.NewJSONHandler(&output, nil))}
	ctx, cancel := context.WithCancel(context.WithValue(t.Context(), joinTelemetryKey{}, telemetry))
	cancel()
	down, up := newFakeSession(), newFakeSession()
	if err := relayWithSessions(ctx, &down, &up); !errors.Is(err, context.Canceled) {
		t.Fatalf("relay cancellation changed: %v", err)
	}
	row := readSessionTerminal(t, &output)
	if row["direction"] != "relay context" || row["cause"] != "cancelled" || row["relay_context"] != "cancelled" {
		t.Fatalf("lost local cancellation cause: %v", row)
	}
}

func TestPreparedConnectionRetainsTerminalAttemptIdentity(t *testing.T) {
	var output bytes.Buffer
	telemetry := &joinTelemetry{attempt: 23, started: time.Now(), logger: slog.New(slog.NewJSONHandler(&output, nil))}
	down, up := newFakeSession(), newFakeUpstream(nil)
	up.reads <- packetResult{err: io.EOF}
	if err := servePreparedConnection(t.Context(), &down, &preparedConnection{upstream: up, telemetry: telemetry}); err != nil {
		t.Fatalf("ordinary terminal changed relay behavior: %v", err)
	}
	row := readSessionTerminal(t, &output)
	if row["attempt_id"] != float64(23) {
		t.Fatalf("handoff lost connection attempt: %v", row)
	}
}

type telemetryReceiveFailure struct{ stage string }

func (failure telemetryReceiveFailure) Error() string        { return "private credential" }
func (failure telemetryReceiveFailure) ReceiveStage() string { return failure.stage }
func (failure telemetryReceiveFailure) PacketID() uint32     { return packet.IDPlaySound }

func TestRelayReportsSafeReceiveMetadata(t *testing.T) {
	for _, stage := range []string{"decoder", "packet", "callback", "private credential"} {
		t.Run(stage, func(t *testing.T) {
			var output bytes.Buffer
			telemetry := &joinTelemetry{started: time.Now(), logger: slog.New(slog.NewJSONHandler(&output, nil))}
			ctx := context.WithValue(t.Context(), joinTelemetryKey{}, telemetry)
			downstream, upstream := newFakeSession(), newFakeSession()
			upstream.reads <- packetResult{err: fmt.Errorf("private credential: %w", telemetryReceiveFailure{stage: stage})}
			_ = relayWithSessions(ctx, &downstream, &upstream)
			row := readSessionTerminal(t, &output)
			want := stage
			if stage == "private credential" {
				want = "unknown"
			}
			if row["receive_stage"] != want || row["protocol_packet_id"] != float64(packet.IDPlaySound) {
				t.Fatalf("lost typed receive metadata: %v", row)
			}
			if strings.Contains(output.String(), "private credential") {
				t.Fatal("receive metadata exposed packet contents")
			}
		})
	}
}

type telemetryTransportClose struct{ reason string }

func (failure telemetryTransportClose) Error() string                            { return "private credential" }
func (failure telemetryTransportClose) Unwrap() error                            { return context.Canceled }
func (failure telemetryTransportClose) TransportCloseReason() string             { return failure.reason }
func (failure telemetryTransportClose) TransportIdleMilliseconds() float64       { return 5120 }
func (failure telemetryTransportClose) TransportRTTMilliseconds() float64        { return 60 }
func (failure telemetryTransportClose) TransportCloseDelayMilliseconds() float64 { return 500 }

func TestRelayReportsSafeTransportCloseMetadata(t *testing.T) {
	for _, reason := range []string{"remote_disconnect", "inactivity_timeout", "local_close", "listener_closed", "dial_cancelled", "raw_read_closed", "raw_read_deadline", "private credential"} {
		t.Run(reason, func(t *testing.T) {
			var output bytes.Buffer
			telemetry := &joinTelemetry{started: time.Now(), logger: slog.New(slog.NewJSONHandler(&output, nil))}
			ctx := context.WithValue(t.Context(), joinTelemetryKey{}, telemetry)
			downstream, upstream := newFakeSession(), newFakeSession()
			upstream.reads <- packetResult{err: fmt.Errorf("private credential: %w", telemetryTransportClose{reason: reason})}
			_ = relayWithSessions(ctx, &downstream, &upstream)
			row := readSessionTerminal(t, &output)
			want := reason
			if reason == "private credential" {
				want = "unknown"
			}
			if row["transport_close_reason"] != want || row["transport_idle_ms"] != float64(5120) || row["transport_rtt_ms"] != float64(60) || row["transport_close_delay_ms"] != float64(500) {
				t.Fatalf("lost transport terminal metadata: %v", row)
			}
			if strings.Contains(output.String(), "private credential") {
				t.Fatal("transport telemetry exposed error contents")
			}
		})
	}
}

func readSessionTerminal(t *testing.T, output *bytes.Buffer) map[string]any {
	t.Helper()
	decoder := json.NewDecoder(bytes.NewReader(output.Bytes()))
	var row map[string]any
	if err := decoder.Decode(&row); err != nil {
		t.Fatalf("missing first session terminal: %v", err)
	}
	if row["msg"] != "SESSION_TERMINAL" {
		t.Fatalf("unexpected session record: %v", row)
	}
	var extra any
	if err := decoder.Decode(&extra); err != io.EOF {
		t.Fatalf("cleanup emitted another terminal: %v, %v", extra, err)
	}
	return row
}
