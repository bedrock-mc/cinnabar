package webrelay

import (
	"bytes"
	"context"
	"crypto/tls"
	"encoding/binary"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"net"
	"net/http"
	"net/http/httptest"
	"net/netip"
	"os"
	"reflect"
	"strings"
	"testing"
	"testing/synctest"
	"time"

	"github.com/coder/websocket"
	"github.com/hashimthearab/rust-mcbe/core/authflow"
	"github.com/hashimthearab/rust-mcbe/core/control"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
	"github.com/quic-go/webtransport-go"
	"golang.org/x/oauth2"
	"golang.org/x/time/rate"
)

// testRelay closes all account state after a test, without logging provider details.
func testRelay(t *testing.T) *Relay {
	t.Helper()
	relay := New(Config{Logger: slog.New(slog.DiscardHandler)})
	t.Cleanup(relay.Close)
	return relay
}

// testWelcome requires a capability response from the current controller.
func testWelcome(t *testing.T, relay *Relay, controller *sessionController, refresh bool, now time.Time) controlEvent {
	t.Helper()
	event, err := relay.welcome(controller, refresh, now)
	if err != nil {
		t.Fatal(err)
	}
	return event
}

func TestResumeRotatesAcrossNetworksAndExpires(t *testing.T) {
	relay := testRelay(t)
	now := time.Now()
	first := netip.MustParseAddr("192.0.2.1")
	second := netip.MustParseAddr("198.51.100.1")
	controller, detach, err := relay.openControl(first, "", now)
	if err != nil {
		t.Fatal(err)
	}
	token := controller.session.token
	generation := controller.generation
	oldDetach := detach
	resumed, detach, err := relay.openControl(second, token, now.Add(time.Second))
	if err != nil || resumed.session != controller.session || resumed.session.token == token || resumed.generation <= generation || relay.perIP[first] != 0 {
		t.Fatal("resume did not rotate and migrate", err)
	}
	oldDetach(now)
	if _, _, err := relay.playSession(nil, hello{Resume: resumed.session.token, Generation: generation}, now); !errors.Is(err, errResume) {
		t.Fatal("stale play generation admitted")
	}
	_, playCtx, err := relay.playSession(nil, hello{Resume: resumed.session.token, Generation: resumed.generation}, now)
	if err != nil {
		t.Fatal(err)
	}
	token = resumed.session.token
	detach(now)
	if playCtx.Err() == nil {
		t.Fatal("closing control did not cancel play")
	}
	if _, _, err := relay.openControl(first, token, now.Add(resumeTTL+time.Second)); !errors.Is(err, errResume) {
		t.Fatal("expired bearer admitted", err)
	}
	if len(relay.sessions) != 1 {
		t.Fatal("refused resume created a new account")
	}
	relay.expire(now.Add(idleTTL + time.Minute))
	if len(relay.sessions) != 0 || len(relay.tokens) != 0 {
		t.Fatal("idle state retained")
	}
}

func TestAdmissionBoundsPendingTransportsAndAccounts(t *testing.T) {
	relay := testRelay(t)
	now := time.Now()
	client := netip.MustParseAddr("192.0.2.1")
	var releases []func()
	for range maxPerIP {
		release, err := relay.admit(client, now)
		if err != nil {
			t.Fatal(err)
		}
		releases = append(releases, release)
	}
	if _, err := relay.admit(client, now); !errors.Is(err, errAdmission) {
		t.Fatal("per-IP transport bound exceeded")
	}
	for _, release := range releases {
		release()
		release()
	}
	for i := range maxSessions {
		addr := netip.AddrFrom4([4]byte{192, 0, byte(i / 256), byte(i % 256)})
		_, _, err := relay.openControl(addr, "", now)
		if err != nil {
			t.Fatal(i, err)
		}
	}
	if _, _, err := relay.openControl(netip.MustParseAddr("198.51.100.1"), "", now); !errors.Is(err, errAdmission) {
		t.Fatal("account bound exceeded")
	}
}

func TestAccountChangesRejectStaleSignInAndSecondPlay(t *testing.T) {
	relay := testRelay(t)
	started, release := make(chan struct{}), make(chan struct{})
	relay.deviceFlow = authflow.DeviceFlow{Authorize: func(context.Context) (*oauth2.DeviceAuthResponse, error) {
		close(started)
		<-release
		return &oauth2.DeviceAuthResponse{VerificationURI: "https://www.microsoft.com/link", UserCode: "TEST"}, nil
	}, Token: func(context.Context, *oauth2.DeviceAuthResponse) (*oauth2.Token, error) {
		return nil, errors.New("cancelled")
	}}
	controller, _, err := relay.openControl(netip.MustParseAddr("192.0.2.1"), "", time.Now())
	if err != nil {
		t.Fatal(err)
	}
	if err = controller.session.SignIn(); err != nil {
		t.Fatal(err)
	}
	<-started
	if err = controller.session.SignOut(); err != nil {
		t.Fatal(err)
	}
	close(release)
	controller.session.signIns.Wait()
	if state, _ := controller.session.Auth(); state.State != control.AuthSignedOut {
		t.Fatal("stale sign-in published after sign-out")
	}
	if _, err = controller.session.beginPlay(t.Context(), controller.ctx, 0); err != nil {
		t.Fatal(err)
	}
	if _, err = controller.session.beginPlay(t.Context(), controller.ctx, 0); !errors.Is(err, errSessionBusy) {
		t.Fatal("second play admitted")
	}
	if err = controller.session.SignOut(); !errors.Is(err, errSessionBusy) {
		t.Fatal("active handler replaced")
	}
	controller.session.endPlay()
}

// bufferConn presents fragmented reads with in-memory writes and no deadline side effects.
type bufferConn struct {
	net.Conn
	reader  io.Reader
	written bytes.Buffer
}

// Read delegates to the fixture's fragmenting reader.
func (conn *bufferConn) Read(p []byte) (int, error) { return conn.reader.Read(p) }

// Write records complete framed responses.
func (conn *bufferConn) Write(p []byte) (int, error) { return conn.written.Write(p) }

// SetReadDeadline accepts codec deadlines without a real clock wait.
func (*bufferConn) SetReadDeadline(time.Time) error { return nil }

// SetWriteDeadline accepts codec deadlines without a real clock wait.
func (*bufferConn) SetWriteDeadline(time.Time) error { return nil }

// frameJSON encodes a fixture using the shared control framing.
func frameJSON(value any) []byte {
	data, _ := json.Marshal(value)
	frame := make([]byte, 4+len(data))
	binary.BigEndian.PutUint32(frame, uint32(len(data)))
	copy(frame[4:], data)
	return frame
}

func TestControlFramingAndContractConformance(t *testing.T) {
	data, err := os.ReadFile("../../crates/bridge/data/contract.json")
	if err != nil {
		t.Fatal(err)
	}
	var manifest struct {
		Relay struct {
			Fixtures []json.RawMessage `json:"fixtures"`
		} `json:"relay"`
	}
	if err = json.Unmarshal(data, &manifest); err != nil {
		t.Fatal(err)
	}
	values := []any{
		hello{Version: bridgecontract.RelayVersion, Channel: bridgecontract.RelayChannelControl, Channels: []string{bridgecontract.RelayChannelControl, bridgecontract.RelayChannelPlay}},
		hello{Version: bridgecontract.RelayVersion, Channel: bridgecontract.RelayChannelPlay, Generation: 1},
		hello{Version: bridgecontract.RelayVersion, Channel: bridgecontract.RelayChannelPlay, Generation: 1, Join: 2},
		controlEvent{Type: bridgecontract.RelayEventWelcome, Version: bridgecontract.RelayVersion, Resume: "test-token", Generation: 1, ExpiresSeconds: int(resumeTTL / time.Second)},
		controlRequest{Op: bridgecontract.RelayOpSignIn},
		controlRequest{Op: bridgecontract.RelayOpPong, Resume: "test-token"},
		controlRequest{Op: bridgecontract.RelayOpSignOut},
		controlEvent{Type: bridgecontract.RelayEventAccount, Account: &control.AuthV1{State: bridgecontract.AuthAwaitingCode, VerificationURI: "https://example.test/verify", UserCode: "TEST-CODE"}},
		controlEvent{Type: bridgecontract.RelayEventConnect, Connect: &proxy.ConnectProgress{Stage: proxy.ConnectStagePacks, PacksDone: 1, PacksTotal: 2, ReceivedBytes: 3, TotalBytes: 4}},
		controlEvent{Type: bridgecontract.RelayEventConnect, Connect: &proxy.ConnectProgress{}},
		controlEvent{Type: bridgecontract.RelayEventError, Error: bridgecontract.RelayRefusalResume},
		controlEvent{Type: bridgecontract.RelayEventError, Error: bridgecontract.RelayRefusalAdmission},
		controlEvent{Type: bridgecontract.RelayEventError, Error: bridgecontract.RelayRefusalSessionBusy},
		controlEvent{Type: bridgecontract.RelayEventError, Error: bridgecontract.RelayRefusalUnknown},
		controlEvent{Type: bridgecontract.RelayEventPing, Version: bridgecontract.RelayVersion, Resume: "test-token", Generation: 1, ExpiresSeconds: int(resumeTTL / time.Second)},
		controlEvent{Type: bridgecontract.RelayEventAccount, Account: &control.AuthV1{State: bridgecontract.AuthSignedOut}},
		controlEvent{Type: bridgecontract.RelayEventConnect, Connect: &proxy.ConnectProgress{Stage: proxy.ConnectStagePacks, PacksDone: 1, PacksTotal: 2, ReceivedBytes: 512, TotalBytes: 1024}},
	}
	if len(values) != len(manifest.Relay.Fixtures) {
		t.Fatal("missing contract fixture")
	}
	for i, value := range values {
		encoded, _ := json.Marshal(value)
		var got, want any
		_ = json.Unmarshal(encoded, &got)
		_ = json.Unmarshal(manifest.Relay.Fixtures[i], &want)
		if !reflect.DeepEqual(got, want) {
			t.Fatalf("wire fixture %d differs", i)
		}
	}
	encoded := append(frameJSON(values[0]), frameJSON(values[1])...)
	fragments := make([]io.Reader, len(encoded))
	for i, b := range encoded {
		fragments[i] = bytes.NewReader([]byte{b})
	}
	conn := &bufferConn{reader: io.MultiReader(fragments...)}
	if _, err = readHello(conn, bridgecontract.RelayChannelControl); err != nil {
		t.Fatal(err)
	}
	if _, err = readHello(conn, bridgecontract.RelayChannelPlay); err != nil {
		t.Fatal(err)
	}
	for _, size := range []uint32{0, bridgecontract.RelayControlBytes + 1} {
		var header [4]byte
		binary.BigEndian.PutUint32(header[:], size)
		if readJSON(&bufferConn{reader: bytes.NewReader(header[:])}, new(hello)) == nil {
			t.Fatal("invalid length admitted")
		}
	}
	if readJSON(&bufferConn{reader: bytes.NewReader(encoded[:len(encoded)-1])}, new(hello)) != nil {
		t.Fatal("complete first frame refused")
	}
	truncated := frameJSON(values[0])
	truncated = truncated[:len(truncated)-1]
	if !errors.Is(readJSON(&bufferConn{reader: bytes.NewReader(truncated)}, new(hello)), io.ErrUnexpectedEOF) {
		t.Fatal("truncated payload admitted")
	}
}

func TestUploadWaitStopsOnSessionCancellation(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		ctx, cancel := context.WithCancel(t.Context())
		limiter := rate.NewLimiter(1, 1)
		if !limiter.Allow() {
			t.Fatal("initial upload burst unavailable")
		}
		conn := &uploadLimitedConn{Conn: &bufferConn{reader: strings.NewReader("x")}, ctx: ctx, limiter: limiter}
		done := make(chan error, 1)
		go func() {
			n, err := conn.Read(make([]byte, 1))
			if n != 0 {
				done <- errors.New("cancelled upload returned bytes")
				return
			}
			done <- err
		}()
		synctest.Wait()
		cancel()
		if err := <-done; !errors.Is(err, context.Canceled) {
			t.Fatal("upload wait ignored session cancellation", err)
		}
		// The transport may already be blocked before its deadline callback runs.
		conn.Conn = &bufferConn{}
		if n, err := conn.Read(make([]byte, 1)); n != 0 || !errors.Is(err, context.Canceled) {
			t.Fatal("cancelled session attempted another transport read", err)
		}
	})
}

func TestWebSocketUsesHelloAndBoundsControlMessages(t *testing.T) {
	relay := testRelay(t)
	server := httptest.NewServer(relay.Handler())
	defer server.Close()
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	socket, _, err := websocket.Dial(ctx, "ws"+strings.TrimPrefix(server.URL, "http")+bridgecontract.RelayControlPath, &websocket.DialOptions{Subprotocols: []string{bridgecontract.RelaySocketProtocol}})
	if err != nil {
		t.Fatal(err)
	}
	defer socket.CloseNow()
	conn := websocket.NetConn(ctx, socket, websocket.MessageBinary)
	if err = writeJSON(context.Background(), conn, hello{Version: bridgecontract.RelayVersion, Channel: bridgecontract.RelayChannelControl, Channels: []string{bridgecontract.RelayChannelControl, bridgecontract.RelayChannelPlay}}); err != nil {
		t.Fatal(err)
	}
	var welcome, account controlEvent
	if err = readJSON(conn, &welcome); err != nil {
		t.Fatal(err)
	}
	if err = readJSON(conn, &account); err != nil {
		t.Fatal(err)
	}
	if welcome.Resume == "" || welcome.Type != bridgecontract.RelayEventWelcome || account.Account == nil || account.Account.State != control.AuthSignedOut {
		t.Fatal("missing Hello/account outcome")
	}

	playSocket, _, err := websocket.Dial(ctx, "ws"+strings.TrimPrefix(server.URL, "http")+bridgecontract.RelayPlayPath, &websocket.DialOptions{Subprotocols: []string{bridgecontract.RelaySocketProtocol}})
	if err != nil {
		t.Fatal(err)
	}
	defer playSocket.CloseNow()
	play := websocket.NetConn(ctx, playSocket, websocket.MessageBinary)
	if err = writeJSON(context.Background(), play, hello{Version: bridgecontract.RelayVersion, Channel: bridgecontract.RelayChannelPlay, Generation: welcome.Generation, Resume: welcome.Resume}); err != nil {
		t.Fatal(err)
	}
	_ = play.SetReadDeadline(time.Now().Add(helloTimeout))
	framed := streamnet.NewFramedConn(play)
	if _, err = framed.Write(append([]byte{bridgecontract.SessionKindConnect}, []byte("{}")...)); err != nil {
		t.Fatal(err)
	}
	frame, err := framed.ReadPacket()
	if err != nil || len(frame) == 0 || frame[0] != bridgecontract.SessionKindDisconnect {
		t.Fatal("core refusal did not traverse fallback play", err)
	}
	message := make([]byte, bridgecontract.RelayControlBytes+5)
	binary.BigEndian.PutUint32(message, bridgecontract.RelayControlBytes)
	copy(message[4:], `{"op":"pong"}`)
	for i := 4 + len(`{"op":"pong"}`); i < len(message)-1; i++ {
		message[i] = ' '
	}
	if err = socket.Write(ctx, websocket.MessageBinary, message); err != nil {
		t.Fatal(err)
	}
	if _, _, err = socket.Read(ctx); websocket.CloseStatus(err) != websocket.StatusMessageTooBig {
		t.Fatal("control message limit was removed by NetConn", err)
	}
}

func TestEgressAndOriginPolicy(t *testing.T) {
	for _, text := range []string{"10.0.0.1", "127.0.0.1", "169.254.169.254", "100.64.0.1", "::1", "fc00::1", "::ffff:127.0.0.1", "64:ff9b::a00:1"} {
		if publicUpstream(netip.MustParseAddr(text)) {
			t.Fatal("private address admitted", text)
		}
	}
	for _, prefix := range reservedPrefixes {
		if publicUpstream(prefix.Addr()) {
			t.Fatal("reserved range admitted", prefix)
		}
	}
	if !publicUpstream(netip.MustParseAddr("8.8.8.8")) {
		t.Fatal("public address refused")
	}
	_, err := newUpstreamDialer(false).DialContext(t.Context(), "udp", "localhost:19132")
	if !errors.Is(err, errBlockedUpstream) {
		t.Fatal("resolved loopback bypassed policy", err)
	}
	if _, err = selectTarget(t.Context(), control.TargetRealm, "1"); err == nil {
		t.Fatal("non-RakNet target admitted")
	}
	if _, err = selectTarget(t.Context(), control.TargetRakNet, "host:0"); err == nil {
		t.Fatal("zero port admitted")
	}
	relay := testRelay(t)
	request := httptest.NewRequest(http.MethodGet, "https://relay.example/", nil)
	request.Header.Set("Origin", "https://other.example")
	if relay.checkOrigin(request) {
		t.Fatal("foreign origin admitted")
	}
	request.Header.Set("Origin", "https://relay.example")
	if !relay.checkOrigin(request) {
		t.Fatal("same host refused")
	}
	request.RemoteAddr = "192.0.2.1:1234"
	request.Header.Set("X-Forwarded-For", "198.51.100.1")
	if clientAddr(request) != netip.MustParseAddr("192.0.2.1") {
		t.Fatal("forwarded header changed admission identity")
	}
}

func TestWebTransportHelloAndReliablePlay(t *testing.T) {
	relay := testRelay(t)
	certificateServer := httptest.NewTLSServer(http.NotFoundHandler())
	certificate := certificateServer.TLS.Certificates[0]
	certificateServer.Close()
	listener, err := net.ListenPacket("udp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	server := relay.transportServer(&tls.Config{MinVersion: tls.VersionTLS13, Certificates: []tls.Certificate{certificate}}, listener.LocalAddr().String())
	done := make(chan error, 1)
	serveCtx, stopServe := context.WithCancel(t.Context())
	go func() { done <- relay.serveQUIC(serveCtx, listener, server) }()
	defer func() { stopServe(); <-done }()
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	defer cancel()
	client := &webtransport.Transport{TLSClientConfig: &tls.Config{InsecureSkipVerify: true}, // Only the local test certificate is untrusted.
		Config: &webtransport.Config{MaxIncomingData: playMessageLimit}}
	defer client.Close()
	_, session, err := client.Dial(ctx, "https://"+listener.LocalAddr().String()+bridgecontract.RelayTransportPath, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.CloseWithError(0, "")
	controlStream, err := session.OpenStreamSync(ctx)
	if err != nil {
		t.Fatal(err)
	}
	controlConn := &transportConn{Stream: controlStream, session: session}
	if err = writeJSON(context.Background(), controlConn, hello{Version: bridgecontract.RelayVersion, Channel: bridgecontract.RelayChannelControl, Channels: []string{bridgecontract.RelayChannelControl, bridgecontract.RelayChannelPlay}}); err != nil {
		t.Fatal(err)
	}
	_ = controlConn.SetReadDeadline(time.Now().Add(helloTimeout))
	var welcome, account controlEvent
	if err = readJSON(controlConn, &welcome); err != nil {
		t.Fatal(err)
	}
	if err = readJSON(controlConn, &account); err != nil {
		t.Fatal(err)
	}
	if welcome.Resume == "" || account.Account == nil || account.Account.State != control.AuthSignedOut {
		t.Fatal("missing control outcome")
	}
	retiring, err := session.OpenStreamSync(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer retiring.CancelRead(0)
	defer retiring.CancelWrite(0)
	// Local cancellation need not have returned credit before the next join opens.
	stream, err := session.OpenStream()
	if err != nil {
		t.Fatal("replacement play could not overlap a retiring stream", err)
	}
	play := &transportConn{Stream: stream, session: session}
	if err = writeJSON(context.Background(), play, hello{Version: bridgecontract.RelayVersion, Channel: bridgecontract.RelayChannelPlay, Generation: welcome.Generation}); err != nil {
		t.Fatal(err)
	}
	_ = play.SetReadDeadline(time.Now().Add(helloTimeout))
	framed := streamnet.NewFramedConn(play)
	if _, err = framed.Write(append([]byte{bridgecontract.SessionKindConnect}, []byte("{}")...)); err != nil {
		t.Fatal(err)
	}
	frame, err := framed.ReadPacket()
	if err != nil || len(frame) == 0 || frame[0] != bridgecontract.SessionKindDisconnect {
		t.Fatal("core refusal did not traverse reliable play", err)
	}
}

func TestPackJoinCannotBeReusedByAController(t *testing.T) {
	relay := testRelay(t)
	controller, detach, err := relay.openControl(netip.MustParseAddr("192.0.2.1"), "", time.Now())
	if err != nil {
		t.Fatal(err)
	}
	defer detach(time.Now())
	first, cancel := context.WithCancel(t.Context())
	defer cancel()
	if !controller.session.claimPackJoin(first, 1) || controller.session.claimPackJoin(first, 1) || controller.session.claimPackJoin(first, 0) || !controller.session.claimPackJoin(first, 2) {
		t.Fatal("join identities were reused")
	}
	next, stop := context.WithCancel(t.Context())
	defer stop()
	if !controller.session.claimPackJoin(next, 1) {
		t.Fatal("new controller could not start a fresh join")
	}
}
