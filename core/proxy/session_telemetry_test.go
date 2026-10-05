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
