package proxy

import (
	"bytes"
	"context"
	"log/slog"
	"strings"
	"testing"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
)

// A local encrypted login exercises the production packet observer without remote accounts.
func TestJoinStageLogsFollowLocalEncryptedLogin(t *testing.T) {
	network := newMemoryNetwork()
	listener, err := (minecraft.ListenConfig{AuthenticationDisabled: true}).ListenNetwork(network, "")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	done := make(chan error, 1)
	go func() {
		accepted, err := listener.Accept()
		if err != nil {
			done <- err
			return
		}
		defer accepted.Close()
		conn := accepted.(*minecraft.Conn)
		err = conn.WritePacketImmediate(relayFixtureStartup()...)
		done <- err
		<-conn.Context().Done()
	}()
	var logs bytes.Buffer
	logger := slog.New(slog.NewTextHandler(&logs, nil))
	dialer := withJoinStages(newUpstreamDialer(dialerTestDownstream{protocol: minecraft.DefaultProtocol}, nil), logger)
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	upstream, err := connectUpstream(ctx, "", "offline", logger, func(ctx context.Context, address string) (upstreamSession, error) {
		return dialer.DialContextNetwork(ctx, network, address)
	})
	if err != nil {
		t.Fatal(err)
	}
	defer upstream.Close()
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	previous := -1
	for _, stage := range []string{"upstream transport connected", "upstream network settings", "upstream login sent", "upstream server-to-client handshake", "upstream encryption enabled", "upstream resource packs info", "upstream resource packs stack", "upstream resource packs done", "upstream StartGame received", "upstream connected"} {
		position := strings.Index(logs.String(), `msg="`+stage+`"`)
		if position <= previous {
			t.Fatalf("stage %q missing or out of order:\n%s", stage, logs.String())
		}
		previous = position
	}
	for _, line := range strings.Split(strings.TrimSpace(logs.String()), "\n") {
		if !strings.Contains(line, "elapsed_ms=") && !strings.Contains(line, "connection starting") {
			t.Fatalf("missing duration: %s", line)
		}
	}
}
