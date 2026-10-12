package webrelay

import (
	"context"
	"encoding/base64"
	"encoding/binary"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"net/netip"
	"strings"
	"testing"
	"time"

	"github.com/coder/websocket"
	"github.com/hashimthearab/rust-mcbe/core/authflow"
	"github.com/hashimthearab/rust-mcbe/core/control"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"golang.org/x/oauth2"
)

// Session message kinds, as core/proxy and the Rust bridge define them.
const (
	kindConnect    = 1
	kindHandoff    = 3
	kindDisconnect = 6
)

func quietLogger() *slog.Logger { return slog.New(slog.NewTextHandler(io.Discard, nil)) }

func startRelay(t *testing.T, cfg Config) (*Relay, *httptest.Server) {
	t.Helper()
	cfg.Logger = quietLogger()
	relay := New(cfg)
	server := httptest.NewServer(relay.Handler())
	t.Cleanup(func() {
		server.CloseClientConnections()
		server.Close()
		relay.Close()
	})
	return relay, server
}

func wsURL(server *httptest.Server, path string) string {
	return "ws" + strings.TrimPrefix(server.URL, "http") + path
}

func dialWS(t *testing.T, url string, header http.Header, protocols ...string) (*websocket.Conn, *http.Response, error) {
	t.Helper()
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	return websocket.Dial(ctx, url, &websocket.DialOptions{Subprotocols: protocols, HTTPHeader: header})
}

func readEvent(t *testing.T, conn *websocket.Conn) controlEvent {
	t.Helper()
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	var event controlEvent
	if err := readJSON(ctx, conn, &event); err != nil {
		t.Fatalf("read control event: %v", err)
	}
	return event
}

// openControl opens a control connection and returns its session ID after the signed-out report.
func openControl(t *testing.T, server *httptest.Server, protocols ...string) (*websocket.Conn, string) {
	t.Helper()
	conn, _, err := dialWS(t, wsURL(server, "/v1/control"), nil, append([]string{ControlProtocol}, protocols...)...)
	if err != nil {
		t.Fatalf("dial control: %v", err)
	}
	t.Cleanup(func() { _ = conn.CloseNow() })
	session := readEvent(t, conn)
	account := readEvent(t, conn)
	if session.Type != "session" || len(session.ID) != 2*sessionIDBytes {
		t.Fatalf("first event = %+v", session)
	}
	if account.Type != "account" || account.Account == nil || account.Account.State != control.AuthSignedOut {
		t.Fatalf("second event = %+v", account)
	}
	return conn, session.ID
}

func TestControlCreatesAndResumesASession(t *testing.T) {
	_, server := startRelay(t, Config{})
	first, id := openControl(t, server)
	_ = first.Close(websocket.StatusNormalClosure, "")
	_, resumed := openControl(t, server, sessionProtocolPrefix+id)
	if resumed != id {
		t.Fatalf("resumed session %q, want %q", resumed[:8], id[:8])
	}
	_, other := openControl(t, server, sessionProtocolPrefix+strings.Repeat("0", 2*sessionIDBytes))
	if other == id {
		t.Fatal("an unknown session ID resumed an existing session")
	}
}

func TestWebSocketsFromOtherOriginsAreRefused(t *testing.T) {
	_, server := startRelay(t, Config{AllowedOrigins: []string{"play.example.com"}})
	header := http.Header{"Origin": {"https://evil.example.net"}}
	if _, response, err := dialWS(t, wsURL(server, "/v1/control"), header, ControlProtocol); err == nil {
		t.Fatal("control accepted a foreign origin")
	} else if response == nil || response.StatusCode != http.StatusForbidden {
		t.Fatalf("foreign origin: response %v, err %v", response, err)
	}
	header.Set("Origin", "https://play.example.com")
	conn, _, err := dialWS(t, wsURL(server, "/v1/control"), header, ControlProtocol)
	if err != nil {
		t.Fatalf("allowed origin: %v", err)
	}
	_ = conn.CloseNow()
}

func TestPlayNeedsTheClientsOwnSession(t *testing.T) {
	relay, server := startRelay(t, Config{})
	_, id := openControl(t, server)
	if _, response, err := dialWS(t, wsURL(server, "/v1/session"), nil, PlayProtocol); err == nil || response.StatusCode != http.StatusForbidden {
		t.Fatalf("play without a session: response %v, err %v", response, err)
	}
	// Bind the session to another client address: its ID alone no longer opens it.
	relay.mu.Lock()
	relay.sessions[id].client = netip.MustParseAddr("203.0.113.9")
	relay.mu.Unlock()
	if _, response, err := dialWS(t, wsURL(server, "/v1/session"), nil, PlayProtocol, sessionProtocolPrefix+id); err == nil || response.StatusCode != http.StatusForbidden {
		t.Fatalf("play from another address: response %v, err %v", response, err)
	}
}

func TestSessionsPerClientAreLimited(t *testing.T) {
	_, server := startRelay(t, Config{MaxSessionsPerIP: 1, ActionsPerMinute: 600})
	openControl(t, server)
	conn, _, err := dialWS(t, wsURL(server, "/v1/control"), nil, ControlProtocol)
	if err != nil {
		t.Fatal(err)
	}
	defer conn.CloseNow()
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	if _, _, err := conn.Read(ctx); websocket.CloseStatus(err) != websocket.StatusTryAgainLater {
		t.Fatalf("second session: %v, want try-again-later", err)
	}
}

func TestSessionCreationIsRateLimited(t *testing.T) {
	relay := New(Config{ActionsPerMinute: 2, Logger: quietLogger()})
	defer relay.Close()
	client := netip.MustParseAddr("198.51.100.7")
	refused := 0
	for range 5 {
		if session, err := relay.createSession(client); err == nil {
			relay.mu.Lock()
			relay.removeLocked(session.id, session)
			relay.mu.Unlock()
			session.close()
		} else if err == errRateLimited {
			refused++
		}
	}
	if refused == 0 {
		t.Fatal("five session creations in a burst of one were all admitted")
	}
}

func TestIdleSessionsExpireAndForgetTheirAccount(t *testing.T) {
	relay := New(Config{IdleTTL: time.Minute, Logger: quietLogger()})
	defer relay.Close()
	session, err := relay.createSession(netip.MustParseAddr("198.51.100.7"))
	if err != nil {
		t.Fatal(err)
	}
	detach, _ := session.attach()
	relay.expire(time.Now().Add(time.Hour))
	if relay.lookup(session.id, session.client) == nil {
		t.Fatal("an attached session expired")
	}
	detach()
	relay.expire(time.Now().Add(2 * time.Minute))
	if relay.lookup(session.id, session.client) != nil {
		t.Fatal("an idle session outlived its TTL")
	}
	if _, err := session.beginPlay(); err == nil {
		t.Fatal("an expired session still plays")
	}
}

// A sign-in abandoned by SignOut neither shows its prompt nor reports its outcome afterwards.
func TestSignOutDiscardsTheAbandonedSignIn(t *testing.T) {
	relay := New(Config{Logger: quietLogger()})
	defer relay.Close()
	release := make(chan struct{})
	relay.deviceFlow = authflow.DeviceFlow{
		Authorize: func(context.Context) (*oauth2.DeviceAuthResponse, error) {
			<-release
			return &oauth2.DeviceAuthResponse{UserCode: "STALE", VerificationURI: "https://example.com/link"}, nil
		},
		Token: func(context.Context, *oauth2.DeviceAuthResponse) (*oauth2.Token, error) {
			return nil, errors.New("denied")
		},
	}
	session, err := relay.createSession(netip.MustParseAddr("198.51.100.7"))
	if err != nil {
		t.Fatal(err)
	}
	if err := session.SignIn(); err != nil {
		t.Fatal(err)
	}
	if err := session.SignOut(); err != nil {
		t.Fatal(err)
	}
	close(release)
	session.signIns.Wait()
	if state, _ := session.Auth(); state.State != control.AuthSignedOut {
		t.Fatalf("account state %q after sign-out, want signed out", state.State)
	}
}

// joinFrames opens a play connection, sends Connect for target and returns the frames that follow.
func joinFrames(t *testing.T, server *httptest.Server, id, target string) <-chan []byte {
	t.Helper()
	conn, _, err := dialWS(t, wsURL(server, "/v1/session"), nil, PlayProtocol, sessionProtocolPrefix+id)
	if err != nil {
		t.Fatalf("dial play: %v", err)
	}
	t.Cleanup(func() { _ = conn.CloseNow() })
	stream := websocket.NetConn(t.Context(), conn, websocket.MessageBinary)
	framed := streamnet.NewFramedConn(stream)
	clientData, _ := json.Marshal(login.ClientData{
		DeviceOS: 7, GameVersion: minecraft.DefaultProtocol.Ver(), LanguageCode: "en_US", SkinID: "skin",
		SkinData:       base64.StdEncoding.EncodeToString(make([]byte, 64*32*4)),
		SkinImageWidth: 64, SkinImageHeight: 32, ThirdPartyName: "Steve",
	})
	connect, _ := json.Marshal(map[string]any{
		"protocol": minecraft.DefaultProtocol.ID(), "client_cache": false, "client_data": json.RawMessage(clientData),
		"target": map[string]string{"kind": control.TargetRakNet, "value": target},
	})
	if _, err := framed.Write(append([]byte{kindConnect}, connect...)); err != nil {
		t.Fatalf("send connect: %v", err)
	}
	frames := make(chan []byte, 16)
	go func() {
		defer close(frames)
		for {
			frame, err := framed.ReadPacket()
			if err != nil {
				return
			}
			frames <- frame
		}
	}()
	return frames
}

func nextFrame(t *testing.T, frames <-chan []byte) []byte {
	t.Helper()
	select {
	case frame, ok := <-frames:
		if !ok {
			t.Fatal("play connection closed")
		}
		return frame
	case <-time.After(20 * time.Second):
		t.Fatal("no frame")
	}
	return nil
}

func listenUpstream(t *testing.T) *minecraft.Listener {
	t.Helper()
	listener, err := minecraft.ListenConfig{
		AuthenticationDisabled: true,
		ErrorLog:               quietLogger(),
	}.Listen("raknet", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("listen: %v", err)
	}
	t.Cleanup(func() { _ = listener.Close() })
	go func() {
		accepted, err := listener.Accept()
		if err != nil {
			return
		}
		_ = accepted.(*minecraft.Conn).WritePacketImmediate(&packet.StartGame{EntityRuntimeID: 9})
	}()
	return listener
}

func TestPlayJoinsAServerOverWebSockets(t *testing.T) {
	upstream := listenUpstream(t)
	_, server := startRelay(t, Config{AllowPrivateUpstream: true})
	_, id := openControl(t, server)
	frames := joinFrames(t, server, id, upstream.Addr().String())
	handoff := nextFrame(t, frames)
	if handoff[0] != kindHandoff {
		t.Fatalf("first frame kind %d, want the handoff", handoff[0])
	}
	length := binary.BigEndian.Uint32(handoff[1:5])
	var body struct {
		Identity struct {
			DisplayName string `json:"display_name"`
		} `json:"identity"`
	}
	if err := json.Unmarshal(handoff[5:5+length], &body); err != nil || body.Identity.DisplayName != "Steve" {
		t.Fatalf("handoff = %s (%v)", handoff[5:5+length], err)
	}
}

// A play message over the upload bound closes the connection instead of reaching the session.
func TestPlayRefusesOversizedMessages(t *testing.T) {
	_, server := startRelay(t, Config{})
	_, id := openControl(t, server)
	conn, _, err := dialWS(t, wsURL(server, "/v1/session"), nil, PlayProtocol, sessionProtocolPrefix+id)
	if err != nil {
		t.Fatalf("dial play: %v", err)
	}
	defer conn.CloseNow()
	// One well-formed session frame whose message is just over the bound.
	message := make([]byte, 4+playMessageLimit)
	binary.BigEndian.PutUint32(message, playMessageLimit)
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	defer cancel()
	go func() { _ = conn.Write(ctx, websocket.MessageBinary, message) }()
	if _, _, err := conn.Read(ctx); websocket.CloseStatus(err) != websocket.StatusMessageTooBig {
		t.Fatalf("oversized message: %v, want message-too-big", err)
	}
}

func TestPlayRefusesPrivateServers(t *testing.T) {
	upstream := listenUpstream(t)
	_, server := startRelay(t, Config{})
	_, id := openControl(t, server)
	frames := joinFrames(t, server, id, upstream.Addr().String())
	if frame := nextFrame(t, frames); frame[0] != kindDisconnect {
		t.Fatalf("frame kind %d, want a disconnect", frame[0])
	}
}

func TestPlayRefusesNonRakNetTargets(t *testing.T) {
	if _, err := selectTarget(context.Background(), control.TargetRealm, "1"); err == nil {
		t.Fatal("a realm target was admitted")
	}
	if address, err := selectTarget(context.Background(), control.TargetRakNet, " play.example.net:19132 "); err != nil || address != "play.example.net:19132" {
		t.Fatalf("raknet target = %q, %v", address, err)
	}
}
