package webrelay

import (
	"context"
	"crypto/tls"
	"errors"
	"net"
	"net/http"
	"net/http/httptest"
	"net/netip"
	"testing"
	"testing/synctest"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
	"github.com/quic-go/quic-go"
	"github.com/quic-go/quic-go/http3"
	"github.com/quic-go/webtransport-go"
)

// admittedQUICServer serves the production admission path with a local fixture certificate.
func admittedQUICServer(t *testing.T, relay *Relay) string {
	t.Helper()
	certificateServer := httptest.NewTLSServer(http.NotFoundHandler())
	certificate := certificateServer.TLS.Certificates[0]
	certificateServer.Close()
	socket, err := net.ListenPacket("udp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	server := relay.transportServer(&tls.Config{MinVersion: tls.VersionTLS13, Certificates: []tls.Certificate{certificate}}, socket.LocalAddr().String())
	ctx, cancel := context.WithCancel(t.Context())
	done := make(chan error, 1)
	go func() { done <- relay.serveQUIC(ctx, socket, server) }()
	t.Cleanup(func() { cancel(); <-done; _ = socket.Close() })
	return socket.LocalAddr().String()
}

// dialRelayQUIC keeps QUIC active without requiring any HTTP/3 request.
func dialRelayQUIC(ctx context.Context, address string) (*quic.Conn, error) {
	return quic.DialAddr(ctx, address, &tls.Config{InsecureSkipVerify: true, NextProtos: []string{http3.NextProtoH3}}, // Only the local fixture's certificate is untrusted.
		&quic.Config{EnableDatagrams: true, EnableStreamResetPartialDelivery: true, KeepAlivePeriod: helloTimeout / 4})
}

func TestQUICAdmissionCountsConnectionsWithoutHTTP(t *testing.T) {
	relay := testRelay(t)
	address := admittedQUICServer(t, relay)
	ctx, cancel := context.WithTimeout(t.Context(), helloTimeout)
	defer cancel()
	var connections []*quic.Conn
	for range maxPerIP {
		conn, err := dialRelayQUIC(ctx, address)
		if err != nil {
			t.Fatal(err)
		}
		connections = append(connections, conn)
		t.Cleanup(func() { _ = conn.CloseWithError(0, "") })
	}
	if activeConnections(relay) != maxPerIP {
		t.Fatal("QUIC peers without HTTP requests were not counted")
	}
	if extra, err := dialRelayQUIC(ctx, address); err == nil {
		_ = extra.CloseWithError(0, "")
		t.Fatal("QUIC connection admission exceeded its per-IP cap")
	} else if timeout, ok := err.(net.Error); ok && timeout.Timeout() {
		t.Fatal("excess QUIC peer timed out instead of being refused", err)
	}
	_ = connections[0].CloseWithError(0, "")
	testwait.Eventually(t, testwait.DefaultTimeout, "QUIC connection teardown", func() bool { return activeConnections(relay) == maxPerIP-1 })
	replacement, err := dialRelayQUIC(ctx, address)
	if err != nil {
		t.Fatal("a closed connection did not return its slot", err)
	}
	defer replacement.CloseWithError(0, "")
	relay.Close()
	if activeConnections(relay) != 0 {
		t.Fatal("relay shutdown retained request-free QUIC connections")
	}
}

func TestQUICAdmissionSharesTheGlobalTransportCap(t *testing.T) {
	relay := testRelay(t)
	address := admittedQUICServer(t, relay)
	for i := range maxSessions {
		release, err := relay.admit(netip.AddrFrom4([4]byte{192, 0, byte(i / 256), byte(i % 256)}), time.Now())
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(release)
	}
	ctx, cancel := context.WithTimeout(t.Context(), helloTimeout)
	defer cancel()
	if conn, err := dialRelayQUIC(ctx, address); err == nil {
		_ = conn.CloseWithError(0, "")
		t.Fatal("QUIC admission bypassed the shared global transport cap")
	} else if timeout, ok := err.(net.Error); ok && timeout.Timeout() {
		t.Fatal("global admission waited for a handshake timeout", err)
	}
	if activeConnections(relay) != maxSessions {
		t.Fatal("a refused QUIC handshake changed the global count")
	}
}

func TestQUICAdmissionRetainsHandshakesAndHandlers(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		relay := testRelay(t)
		ctx, cancel := context.WithCancel(t.Context())
		defer cancel()
		ctx, err := relay.quicConnContext(ctx, &quic.ClientInfo{RemoteAddr: &net.UDPAddr{IP: net.IPv4(192, 0, 2, 1), Port: 1234}})
		if err != nil {
			t.Fatal(err)
		}
		admission := ctx.Value(quicAdmissionKey{}).(*admittedQUICConn)
		workerDone, err := admission.retain()
		if err != nil {
			t.Fatal(err)
		}
		defer workerDone()
		request := httptest.NewRequest(http.MethodConnect, "https://relay.example/", nil).WithContext(ctx)
		_, handlerDone, err := relay.admitTransport(request)
		if err != nil {
			t.Fatal(err)
		}
		defer handlerDone()
		if _, finish, err := relay.admitTransport(request); !errors.Is(err, errAdmission) {
			if finish != nil {
				finish()
			}
			t.Fatal("one QUIC connection opened multiple WebTransport sessions", err)
		}
		cancel()
		synctest.Wait()
		workerDone()
		if activeConnections(relay) != 1 {
			t.Fatal("connection teardown released a running WebTransport handler")
		}
		handlerDone()
		handlerDone()
		if activeConnections(relay) != 0 {
			t.Fatal("QUIC admission did not release exactly once")
		}
	})
}

func TestQUICFailedHandshakeReturnsAdmission(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		relay := testRelay(t)
		ctx, cancel := context.WithCancel(t.Context())
		defer cancel()
		_, err := relay.quicConnContext(ctx, &quic.ClientInfo{RemoteAddr: &net.UDPAddr{IP: net.IPv4(192, 0, 2, 1), Port: 1234}})
		if err != nil || activeConnections(relay) != 1 {
			t.Fatal("pending handshake was not admitted", err)
		}
		cancel()
		synctest.Wait()
		if activeConnections(relay) != 0 {
			t.Fatal("failed handshake retained a connection slot")
		}
	})
}

func TestQUICInitialDeadlineCoversEveryPreHelloStage(t *testing.T) {
	for _, stage := range []string{"ping_only", "stream_prefix", "uni_prefix", "http_headers", "no_control_stream", "control_header"} {
		t.Run(stage, func(t *testing.T) {
			relay := testRelay(t)
			address := admittedQUICServer(t, relay)
			ctx, cancel := context.WithTimeout(t.Context(), helloTimeout+testwait.DefaultTimeout)
			defer cancel()
			conn, err := dialRelayQUIC(ctx, address)
			if err != nil {
				t.Fatal(err)
			}
			defer conn.CloseWithError(0, "")
			switch stage {
			case "uni_prefix":
				stream, err := conn.OpenUniStreamSync(ctx)
				if err != nil {
					t.Fatal(err)
				}
				if _, err := stream.Write([]byte{0x40}); err != nil {
					t.Fatal(err)
				}
			case "stream_prefix", "http_headers":
				stream, err := conn.OpenStreamSync(ctx)
				if err != nil {
					t.Fatal(err)
				}
				data := []byte{0x40} // Incomplete QUIC variable-length stream prefix.
				if stage == "http_headers" {
					data = []byte{1, 16, 0} // HEADERS with an incomplete field section.
				}
				if _, err := stream.Write(data); err != nil {
					t.Fatal(err)
				}
			case "no_control_stream", "control_header":
				client := &webtransport.Transport{}
				clientConn, err := client.NewClientConn(conn)
				if err != nil {
					t.Fatal(err)
				}
				defer client.Close()
				_, session, err := clientConn.Dial(ctx, "https://"+address+bridgecontract.RelayTransportPath, nil)
				if err != nil {
					t.Fatal(err)
				}
				defer session.CloseWithError(0, "")
				if stage == "control_header" {
					stream, err := session.OpenStreamSync(ctx)
					if err != nil {
						t.Fatal(err)
					}
					if _, err := stream.Write([]byte{0, 0, 0, 4}); err != nil {
						t.Fatal(err)
					}
				}
			}
			select {
			case <-conn.Context().Done():
			case <-ctx.Done():
				t.Fatal("QUIC PINGs kept a pre-Hello connection alive", stage)
			}
			testwait.Eventually(t, testwait.DefaultTimeout, "initial connection teardown", func() bool { return activeConnections(relay) == 0 })
		})
	}
}

func TestWebTransportCannotOpenASecondSessionOnOneConnection(t *testing.T) {
	relay := testRelay(t)
	address := admittedQUICServer(t, relay)
	ctx, cancel := context.WithTimeout(t.Context(), helloTimeout)
	defer cancel()
	conn, err := dialRelayQUIC(ctx, address)
	if err != nil {
		t.Fatal(err)
	}
	defer conn.CloseWithError(0, "")
	client := &webtransport.Transport{}
	clientConn, err := client.NewClientConn(conn)
	if err != nil {
		t.Fatal(err)
	}
	defer client.Close()
	url := "https://" + address + bridgecontract.RelayTransportPath
	_, session, err := clientConn.Dial(ctx, url, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.CloseWithError(0, "")
	stream, err := session.OpenStreamSync(ctx)
	if err != nil {
		t.Fatal(err)
	}
	control := &transportConn{Stream: stream, session: session}
	if err := writeJSON(context.Background(), control, hello{Version: bridgecontract.RelayVersion, Channel: bridgecontract.RelayChannelControl,
		Channels: []string{bridgecontract.RelayChannelControl, bridgecontract.RelayChannelPlay}}); err != nil {
		t.Fatal(err)
	}
	for range 2 {
		if err := readJSON(control, new(controlEvent)); err != nil {
			t.Fatal(err)
		}
	}
	response, extra, err := clientConn.Dial(ctx, url, nil)
	if extra != nil {
		_ = extra.CloseWithError(0, "")
		t.Fatal("second WebTransport session was admitted")
	}
	if err == nil || response == nil || response.StatusCode != http.StatusTooManyRequests {
		t.Fatal("second session did not receive admission refusal", err)
	}
	_ = response.Body.Close()
	if activeConnections(relay) != 1 {
		t.Fatal("a WebTransport upgrade changed the underlying connection count")
	}
}
