package webrelay

import (
	"context"
	"crypto/tls"
	"net"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/coder/websocket"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
	"github.com/quic-go/webtransport-go"
)

// takeoverDialer opens independent browser transports against a local relay.
// Each returned browser can open its control and play channels on the same transport.
func takeoverDialer(t *testing.T, relay *Relay, quic bool) func() func(string) net.Conn {
	t.Helper()
	ctx, cancel := context.WithTimeout(t.Context(), testwait.DefaultTimeout)
	t.Cleanup(cancel)
	if !quic {
		server := httptest.NewServer(relay.Handler())
		t.Cleanup(server.Close)
		return func() func(string) net.Conn {
			return func(channel string) net.Conn {
				t.Helper()
				path := bridgecontract.RelayControlPath
				if channel == bridgecontract.RelayChannelPlay {
					path = bridgecontract.RelayPlayPath
				}
				socket, _, err := websocket.Dial(ctx, "ws"+strings.TrimPrefix(server.URL, "http")+path,
					&websocket.DialOptions{Subprotocols: []string{bridgecontract.RelaySocketProtocol}})
				if err != nil {
					t.Fatal(err)
				}
				t.Cleanup(func() { _ = socket.CloseNow() })
				return websocket.NetConn(ctx, socket, websocket.MessageBinary)
			}
		}
	}
	certificateServer := httptest.NewTLSServer(http.NotFoundHandler())
	certificate := certificateServer.TLS.Certificates[0]
	certificateServer.Close()
	listener, err := net.ListenPacket("udp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	server := relay.transportServer(&tls.Config{MinVersion: tls.VersionTLS13, Certificates: []tls.Certificate{certificate}}, listener.LocalAddr().String())
	serveCtx, stop := context.WithCancel(ctx)
	done := make(chan error, 1)
	go func() { done <- relay.serveQUIC(serveCtx, listener, server) }()
	t.Cleanup(func() { stop(); <-done; _ = listener.Close() })
	return func() func(string) net.Conn {
		t.Helper()
		client := &webtransport.Transport{TLSClientConfig: &tls.Config{InsecureSkipVerify: true}} // Local test certificate only.
		t.Cleanup(func() { _ = client.Close() })
		_, transport, err := client.Dial(ctx, "https://"+listener.LocalAddr().String()+bridgecontract.RelayTransportPath, nil)
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = transport.CloseWithError(0, "") })
		return func(string) net.Conn {
			t.Helper()
			stream, err := transport.OpenStreamSync(ctx)
			if err != nil {
				t.Fatal(err)
			}
			conn := &transportConn{Stream: stream, session: transport}
			_ = conn.SetDeadline(time.Now().Add(testwait.DefaultTimeout))
			return conn
		}
	}
}

// takeoverControl sends Hello and requires a successful welcome and account response.
func takeoverControl(t *testing.T, conn net.Conn, token string) controlEvent {
	t.Helper()
	if err := writeJSON(t.Context(), conn, hello{Version: bridgecontract.RelayVersion, Channel: bridgecontract.RelayChannelControl,
		Channels: []string{bridgecontract.RelayChannelControl, bridgecontract.RelayChannelPlay}, Resume: token}); err != nil {
		t.Fatal(err)
	}
	var welcome, account controlEvent
	if err := readJSON(conn, &welcome); err != nil {
		t.Fatal(err)
	}
	if welcome.Type != bridgecontract.RelayEventWelcome || welcome.Resume == "" {
		t.Fatalf("valid resume did not receive a welcome: %s %s", welcome.Type, welcome.Error)
	}
	if err := readJSON(conn, &account); err != nil || account.Account == nil {
		t.Fatal("missing retained account response", err)
	}
	return welcome
}

func TestResumeTakesOverLiveTransport(t *testing.T) {
	for _, quic := range []bool{false, true} {
		t.Run(map[bool]string{false: "websocket", true: "quic"}[quic], func(t *testing.T) {
			relay := testRelay(t)
			dial := takeoverDialer(t, relay, quic)
			oldBrowser := dial()
			oldControl := oldBrowser(bridgecontract.RelayChannelControl)
			welcome := takeoverControl(t, oldControl, "")
			relay.mu.Lock()
			session := relay.tokens[welcome.Resume].session
			relay.mu.Unlock()
			oldPlay := oldBrowser(bridgecontract.RelayChannelPlay)
			if err := writeJSON(t.Context(), oldPlay, hello{Version: bridgecontract.RelayVersion, Channel: bridgecontract.RelayChannelPlay,
				Resume: welcome.Resume, Generation: welcome.Generation}); err != nil {
				t.Fatal(err)
			}
			testwait.Eventually(t, testwait.DefaultTimeout, "old play handler", func() bool {
				session.mu.Lock()
				defer session.mu.Unlock()
				return session.play != nil
			})
			browser := dial()
			conn := browser(bridgecontract.RelayChannelControl)
			resumed := takeoverControl(t, conn, welcome.Resume)
			if resumed.Generation <= welcome.Generation || resumed.Resume == welcome.Resume {
				t.Fatal("takeover did not advance the controller and bearer")
			}
			if err := readJSON(oldControl, new(controlEvent)); err == nil {
				t.Fatal("old control connection survived takeover")
			}
			if _, err := oldPlay.Read(make([]byte, 1)); err == nil {
				t.Fatal("old play connection survived takeover")
			}
			testwait.Eventually(t, testwait.DefaultTimeout, "old controller and play teardown", func() bool {
				session.mu.Lock()
				defer session.mu.Unlock()
				return session.attached == 1 && session.play == nil
			})
			play := browser(bridgecontract.RelayChannelPlay)
			if err := writeJSON(t.Context(), play, hello{Version: bridgecontract.RelayVersion, Channel: bridgecontract.RelayChannelPlay,
				Resume: resumed.Resume, Generation: resumed.Generation}); err != nil {
				t.Fatal(err)
			}
			framed := streamnet.NewFramedConn(play)
			if _, err := framed.Write(append([]byte{bridgecontract.SessionKindConnect}, []byte("{}")...)); err != nil {
				t.Fatal(err)
			}
			frame, err := framed.ReadPacket()
			if err != nil || len(frame) == 0 || frame[0] != bridgecontract.SessionKindDisconnect {
				t.Fatal("new play generation failed after old teardown", err)
			}
		})
	}
}
