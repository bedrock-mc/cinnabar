package webrelay

import (
	"context"
	"crypto/tls"
	"errors"
	"net"
	"net/http"
	"net/http/httptest"
	"net/netip"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/coder/websocket"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
)

// admittedTLSServer applies the production accept boundary to a local TLS server.
func admittedTLSServer(t *testing.T, relay *Relay) *httptest.Server {
	t.Helper()
	server := httptest.NewUnstartedServer(relay.Handler())
	server.Listener = &admittedTCPListener{Listener: server.Listener, relay: relay}
	server.Config = relay.socketServer(nil, "")
	server.StartTLS()
	t.Cleanup(server.Close)
	return server
}

func TestFallbackDoesNotNegotiateHTTP2(t *testing.T) {
	relay := testRelay(t)
	fixture := httptest.NewTLSServer(http.NotFoundHandler())
	certificate := fixture.TLS.Certificates[0]
	fixture.Close()
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	server := relay.socketServer(&tls.Config{MinVersion: tls.VersionTLS13, Certificates: []tls.Certificate{certificate}}, listener.Addr().String())
	done := make(chan error, 1)
	go func() { done <- server.ServeTLS(&admittedTCPListener{Listener: listener, relay: relay}, "", "") }()
	t.Cleanup(func() { _ = server.Close(); <-done })
	ctx, cancel := context.WithTimeout(t.Context(), helloTimeout)
	defer cancel()
	dialer := tls.Dialer{Config: &tls.Config{InsecureSkipVerify: true, NextProtos: []string{"h2", "http/1.1"}}} // Only the local fixture's certificate is untrusted.
	conn, err := dialer.DialContext(ctx, "tcp", listener.Addr().String())
	if err != nil {
		t.Fatal(err)
	}
	defer conn.Close()
	if conn.(*tls.Conn).ConnectionState().NegotiatedProtocol != "http/1.1" {
		t.Fatal("fallback admitted a multiplexed HTTP/2 connection")
	}
}

// activeConnections reads the admission count under its owning lock.
func activeConnections(relay *Relay) int {
	relay.mu.Lock()
	defer relay.mu.Unlock()
	return relay.active
}

func TestTCPAdmissionCountsPeersBeforeTLSAndHTTP(t *testing.T) {
	for _, handshake := range []bool{false, true} {
		t.Run(map[bool]string{false: "before_tls", true: "before_http"}[handshake], func(t *testing.T) {
			relay := testRelay(t)
			server := admittedTLSServer(t, relay)
			ctx, cancel := context.WithTimeout(t.Context(), helloTimeout)
			defer cancel()
			for range maxPerIP {
				conn, err := (&net.Dialer{}).DialContext(ctx, "tcp", server.Listener.Addr().String())
				if err != nil {
					t.Fatal(err)
				}
				t.Cleanup(func() { _ = conn.Close() })
				if handshake {
					secure := tls.Client(conn, &tls.Config{InsecureSkipVerify: true}) // Only the local fixture's certificate is untrusted.
					if err := secure.HandshakeContext(ctx); err != nil {
						t.Fatal(err)
					}
				}
			}
			testwait.Eventually(t, testwait.DefaultTimeout, "accept-time admission", func() bool { return activeConnections(relay) == maxPerIP })
			extra, err := (&net.Dialer{}).DialContext(ctx, "tcp", server.Listener.Addr().String())
			if err != nil {
				t.Fatal(err)
			}
			defer extra.Close()
			_ = extra.SetReadDeadline(time.Now().Add(helloTimeout))
			if _, err := extra.Read(make([]byte, 1)); err == nil {
				t.Fatal("an excess peer was not closed")
			} else if timeout, ok := err.(net.Error); ok && timeout.Timeout() {
				t.Fatal("an excess peer remained open until its deadline", err)
			}
			if activeConnections(relay) != maxPerIP {
				t.Fatal("refused TCP socket changed the admission count")
			}
			relay.Close()
			if activeConnections(relay) != 0 {
				t.Fatal("relay shutdown retained pre-upgrade sockets")
			}
		})
	}
}

func TestTCPAdmissionSharesGlobalTransportCap(t *testing.T) {
	relay := testRelay(t)
	server := admittedTLSServer(t, relay)
	for i := range maxSessions {
		client := netip.AddrFrom4([4]byte{192, 0, byte(i / 256), byte(i % 256)})
		release, err := relay.admit(client, time.Now())
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(release)
	}
	ctx, cancel := context.WithTimeout(t.Context(), helloTimeout)
	defer cancel()
	conn, err := (&net.Dialer{}).DialContext(ctx, "tcp", server.Listener.Addr().String())
	if err != nil {
		t.Fatal(err)
	}
	defer conn.Close()
	_ = conn.SetReadDeadline(time.Now().Add(helloTimeout))
	if _, err := conn.Read(make([]byte, 1)); err == nil {
		t.Fatal("TCP admission exceeded the shared transport cap")
	} else if timeout, ok := err.(net.Error); ok && timeout.Timeout() {
		t.Fatal("TCP peer waited for a deadline instead of being refused", err)
	}
}

func TestTCPWebSocketUpgradeRetainsOneAdmission(t *testing.T) {
	relay := testRelay(t)
	server := admittedTLSServer(t, relay)
	ctx, cancel := context.WithTimeout(t.Context(), helloTimeout)
	defer cancel()
	socket, _, err := websocket.Dial(ctx, "wss"+strings.TrimPrefix(server.URL, "https")+bridgecontract.RelayControlPath,
		&websocket.DialOptions{HTTPClient: server.Client(), Subprotocols: []string{bridgecontract.RelaySocketProtocol}})
	if err != nil {
		t.Fatal(err)
	}
	defer socket.CloseNow()
	conn := websocket.NetConn(ctx, socket, websocket.MessageBinary)
	if err := writeJSON(context.Background(), conn, hello{Version: bridgecontract.RelayVersion, Channel: bridgecontract.RelayChannelControl,
		Channels: []string{bridgecontract.RelayChannelControl, bridgecontract.RelayChannelPlay}}); err != nil {
		t.Fatal(err)
	}
	for range 2 {
		if err := readJSON(conn, new(controlEvent)); err != nil {
			t.Fatal(err)
		}
	}
	if activeConnections(relay) != 1 {
		t.Fatal("WebSocket upgrade charged its TCP connection twice")
	}
	_ = socket.CloseNow()
	testwait.Eventually(t, testwait.DefaultTimeout, "upgraded handler teardown", func() bool { return activeConnections(relay) == 0 })
}

func TestTCPAdmissionWaitsForSocketAndHandler(t *testing.T) {
	relay := testRelay(t)
	release, err := relay.admit(netip.MustParseAddr("192.0.2.1"), time.Now())
	if err != nil {
		t.Fatal(err)
	}
	server, peer := net.Pipe()
	defer peer.Close()
	conn := &admittedTCPConn{Conn: server, relay: relay, connectionAdmission: newConnectionAdmission(release)}
	defer conn.Close()
	finish, err := conn.retain()
	if err != nil {
		t.Fatal(err)
	}
	defer finish()
	_ = conn.Close()
	if activeConnections(relay) != 1 {
		t.Fatal("closing the socket released its still-running handler")
	}
	if extra, err := conn.retain(); !errors.Is(err, net.ErrClosed) {
		if extra != nil {
			extra()
		}
		t.Fatal("closed socket admitted another handler", err)
	}
	finish()
	finish()
	_ = conn.Close()
	if activeConnections(relay) != 0 {
		t.Fatal("socket and handler teardown did not release admission once")
	}
}

func TestTCPAdmissionWaitsForAnOrdinaryHTTPHandler(t *testing.T) {
	relay := testRelay(t)
	entered, handlerDone := make(chan struct{}), make(chan struct{})
	finish := sync.OnceFunc(func() { close(handlerDone) })
	defer finish()
	server := httptest.NewUnstartedServer(nil)
	server.Listener = &admittedTCPListener{Listener: server.Listener, relay: relay}
	server.Config = relay.socketServer(nil, "")
	server.Config.Handler = http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		close(entered)
		<-handlerDone
		w.WriteHeader(http.StatusNoContent)
	})
	server.StartTLS()
	t.Cleanup(server.Close)
	ctx, cancel := context.WithTimeout(t.Context(), helloTimeout)
	defer cancel()
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, server.URL, nil)
	if err != nil {
		t.Fatal(err)
	}
	requestDone := make(chan struct{})
	go func() {
		defer close(requestDone)
		response, err := server.Client().Do(request)
		if err == nil {
			_ = response.Body.Close()
		}
	}()
	select {
	case <-entered:
	case <-requestDone:
		t.Fatal("HTTP request ended before entering its handler")
	case <-ctx.Done():
		t.Fatal("HTTP handler did not start")
	}
	relay.cancel()
	select {
	case <-requestDone:
	case <-ctx.Done():
		t.Fatal("socket closure did not end the peer's request")
	}
	if activeConnections(relay) != 1 {
		t.Fatal("socket closure released a running HTTP handler")
	}
	finish()
	testwait.Eventually(t, testwait.DefaultTimeout, "ordinary HTTP handler teardown", func() bool { return activeConnections(relay) == 0 })
}

func TestFallbackIncompleteHTTPBodyCannotHoldAdmission(t *testing.T) {
	relay := testRelay(t)
	server := admittedTLSServer(t, relay)
	ctx, cancel := context.WithTimeout(t.Context(), helloTimeout)
	defer cancel()
	dialer := tls.Dialer{Config: &tls.Config{InsecureSkipVerify: true}} // Only the local fixture's certificate is untrusted.
	conn, err := dialer.DialContext(ctx, "tcp", server.Listener.Addr().String())
	if err != nil {
		t.Fatal(err)
	}
	defer conn.Close()
	if _, err := conn.Write([]byte("POST / HTTP/1.1\r\nHost: relay.example\r\nContent-Length: 1\r\n\r\n")); err != nil {
		t.Fatal(err)
	}
	testwait.Eventually(t, helloTimeout+testwait.DefaultTimeout, "incomplete HTTP body teardown", func() bool { return activeConnections(relay) == 0 })
}
