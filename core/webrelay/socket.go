package webrelay

import (
	"context"
	"encoding/json"
	"errors"
	"net"
	"net/http"
	"strings"
	"time"

	"github.com/coder/websocket"
	"github.com/hashimthearab/rust-mcbe/core/control"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
	"golang.org/x/time/rate"
)

// WebSocket subprotocols. A browser names its session with a second subprotocol,
// sessionProtocolPrefix + ID, so the ID never appears in a URL or a log line.
const (
	ControlProtocol       = "cinnabar.control.v1"
	PlayProtocol          = "cinnabar.session.v1"
	sessionProtocolPrefix = "sid."
)

const (
	controlReadLimit  = 4 << 10
	playMessageLimit  = 8 << 20 // one session frame per message; Connect carries the skin
	keepAliveInterval = 30 * time.Second
)

// controlRequest is a browser's control message.
type controlRequest struct {
	Op string `json:"op"` // "sign_in" or "sign_out"
}

// controlEvent is a relay control message: "session" names the session, "account" reports its
// account state, "connect" its join's stage and pack download progress, and "error" a refused
// request.
type controlEvent struct {
	Type    string                 `json:"type"`
	ID      string                 `json:"id,omitempty"`
	Account *control.AuthV1        `json:"account,omitempty"`
	Connect *proxy.ConnectProgress `json:"connect,omitempty"`
	Error   string                 `json:"error,omitempty"`
}

// connectEventInterval spaces join progress reports, which pack chunks would otherwise flood.
const connectEventInterval = 100 * time.Millisecond

// requestedSession returns the session ID a WebSocket handshake names, if any.
func requestedSession(r *http.Request) string {
	for _, header := range r.Header.Values("Sec-WebSocket-Protocol") {
		for _, protocol := range strings.Split(header, ",") {
			if id, ok := strings.CutPrefix(strings.TrimSpace(protocol), sessionProtocolPrefix); ok {
				return id
			}
		}
	}
	return ""
}

func (relay *Relay) accept(w http.ResponseWriter, r *http.Request, protocol string) (*websocket.Conn, error) {
	conn, err := websocket.Accept(w, r, &websocket.AcceptOptions{
		Subprotocols:   []string{protocol},
		OriginPatterns: relay.cfg.AllowedOrigins,
	})
	if err != nil {
		return nil, err
	}
	if conn.Subprotocol() != protocol {
		_ = conn.Close(websocket.StatusPolicyViolation, "unsupported protocol")
		return nil, errors.New("webrelay: unsupported protocol")
	}
	return conn, nil
}

// serveControl resumes or creates a session, then reports its account and runs sign-in requests.
func (relay *Relay) serveControl(w http.ResponseWriter, r *http.Request) {
	client := relay.clientAddr(r)
	conn, err := relay.accept(w, r, ControlProtocol)
	if err != nil {
		return
	}
	relay.conns.Add(1)
	defer relay.conns.Done()
	conn.SetReadLimit(controlReadLimit)
	ctx, cancel := context.WithCancel(relay.ctx)
	defer cancel()

	session := relay.lookup(requestedSession(r), client)
	if session == nil {
		if session, err = relay.createSession(client); err != nil {
			_ = conn.Close(websocket.StatusTryAgainLater, refusal(err))
			return
		}
	}
	detach, err := session.attach()
	if err != nil {
		_ = conn.Close(websocket.StatusGoingAway, "session closed")
		return
	}
	defer detach()

	events := make(chan controlEvent, 4)
	go func() {
		defer cancel()
		for {
			var request controlRequest
			if err := readJSON(ctx, conn, &request); err != nil {
				return
			}
			var opErr error
			switch request.Op {
			case "sign_in":
				if !relay.allow(client) {
					opErr = errRateLimited
				} else {
					opErr = session.SignIn()
				}
			case "sign_out":
				opErr = session.SignOut()
			default:
				opErr = errors.New("unknown op")
			}
			if opErr != nil {
				select {
				case events <- controlEvent{Type: "error", Error: refusal(opErr)}:
				case <-ctx.Done():
					return
				}
			}
		}
	}()

	if writeJSON(ctx, conn, controlEvent{Type: "session", ID: session.id}) != nil {
		return
	}
	keepAlive := time.NewTicker(keepAliveInterval)
	defer keepAlive.Stop()
	state, changed := session.Auth()
	if writeJSON(ctx, conn, controlEvent{Type: "account", Account: &state}) != nil {
		return
	}
	_, connectChanged := session.Connect()
	for {
		var err error
		select {
		case <-changed:
			state, changed = session.Auth()
			err = writeJSON(ctx, conn, controlEvent{Type: "account", Account: &state})
		case <-connectChanged:
			var progress proxy.ConnectProgress
			progress, connectChanged = session.Connect()
			err = writeJSON(ctx, conn, controlEvent{Type: "connect", Connect: &progress})
			if err == nil && progress.Stage != "" {
				// Later changes coalesce into the next report.
				select {
				case <-time.After(connectEventInterval):
				case <-ctx.Done():
				}
			}
		case event := <-events:
			err = writeJSON(ctx, conn, event)
		case <-keepAlive.C:
			err = conn.Ping(ctx)
		case <-ctx.Done():
			_ = conn.Close(websocket.StatusNormalClosure, "")
			return
		}
		if err != nil {
			return
		}
	}
}

// servePlay carries one session connection: the core's session frames over binary messages.
func (relay *Relay) servePlay(w http.ResponseWriter, r *http.Request) {
	client := relay.clientAddr(r)
	session := relay.lookup(requestedSession(r), client)
	if session == nil {
		http.Error(w, "unknown session", http.StatusForbidden)
		return
	}
	conn, err := relay.accept(w, r, PlayProtocol)
	if err != nil {
		return
	}
	relay.conns.Add(1)
	defer relay.conns.Done()
	if !relay.allow(client) {
		_ = conn.Close(websocket.StatusTryAgainLater, refusal(errRateLimited))
		return
	}
	detach, err := session.attach()
	if err != nil {
		_ = conn.Close(websocket.StatusGoingAway, "session closed")
		return
	}
	defer detach()
	handler, err := session.beginPlay()
	if err != nil {
		_ = conn.Close(websocket.StatusTryAgainLater, refusal(err))
		return
	}
	defer session.endPlay()

	ctx, cancel := context.WithCancel(relay.ctx)
	defer cancel()
	stream := websocket.NetConn(ctx, conn, websocket.MessageBinary)
	conn.SetReadLimit(playMessageLimit) // NetConn lifts the limit
	limited := &uploadLimitedConn{Conn: stream, limiter: rate.NewLimiter(rate.Limit(relay.cfg.UploadBytesPerSec), playMessageLimit)}
	if err := handler.ServeConn(ctx, limited); err != nil && ctx.Err() == nil {
		relay.logger.Debug("web play connection ended", "session", session.id[:8], "error", err)
	}
	_ = stream.Close()
}

// uploadLimitedConn paces what the browser sends, so a session cannot turn the relay into a
// high-rate sender toward a server.
type uploadLimitedConn struct {
	net.Conn
	limiter *rate.Limiter
}

func (conn *uploadLimitedConn) Read(p []byte) (int, error) {
	n, err := conn.Conn.Read(p)
	if n > 0 {
		if waitErr := conn.limiter.WaitN(context.Background(), min(n, conn.limiter.Burst())); waitErr != nil && err == nil {
			err = waitErr
		}
	}
	return n, err
}

// refusal is the client-facing reason for err; it never carries addresses or provider detail.
func refusal(err error) string {
	switch {
	case errors.Is(err, errRateLimited):
		return "rate_limited"
	case errors.Is(err, errTooManySessions):
		return "too_many_sessions"
	case errors.Is(err, errSessionBusy):
		return "session_busy"
	default:
		return "refused"
	}
}

func readJSON(ctx context.Context, conn *websocket.Conn, value any) error {
	kind, data, err := conn.Read(ctx)
	if err != nil {
		return err
	}
	if kind != websocket.MessageText {
		return errors.New("webrelay: control messages are text")
	}
	return json.Unmarshal(data, value)
}

func writeJSON(ctx context.Context, conn *websocket.Conn, value any) error {
	data, err := json.Marshal(value)
	if err != nil {
		return err
	}
	writeCtx, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()
	return conn.Write(writeCtx, websocket.MessageText, data)
}
