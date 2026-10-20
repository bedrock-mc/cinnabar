package webrelay

import (
	"context"
	"encoding/binary"
	"encoding/json"
	"errors"
	"io"
	"net"
	"net/netip"
	"slices"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/control"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
	"golang.org/x/time/rate"
)

// hello identifies a reliable channel before any account or play operation.
type hello struct {
	Join       uint64   `json:"join,omitempty"`
	Version    uint32   `json:"version"`
	Channel    string   `json:"channel"`
	Channels   []string `json:"channels,omitempty"`
	Resume     string   `json:"resume,omitempty"`
	Generation uint64   `json:"generation,omitempty"`
}

type controlRequest struct {
	Op     string `json:"op"`
	Resume string `json:"resume,omitempty"`
}

type controlEvent struct {
	Type           string                 `json:"type"`
	Version        uint32                 `json:"version,omitempty"`
	Resume         string                 `json:"resume,omitempty"`
	Generation     uint64                 `json:"generation,omitempty"`
	ExpiresSeconds int                    `json:"expires_seconds,omitempty"`
	Account        *control.AuthV1        `json:"account,omitempty"`
	Connect        *proxy.ConnectProgress `json:"connect,omitempty"`
	Error          string                 `json:"error,omitempty"`
}

const controlRequestBurst = bridgecontract.RelayControlRequestBurst

// readJSON admits a nonzero u32 big-endian JSON payload before allocating it.
func readJSON(conn net.Conn, value any) error {
	var header [4]byte
	if _, err := io.ReadFull(conn, header[:]); err != nil {
		return err
	}
	size := binary.BigEndian.Uint32(header[:])
	if size == 0 || size > bridgecontract.RelayControlBytes {
		return errors.New("webrelay: invalid control length")
	}
	data := make([]byte, size)
	if _, err := io.ReadFull(conn, data); err != nil {
		return err
	}
	return json.Unmarshal(data, value)
}

// writeJSON bounds every control response and the time spent writing it.
// It checks ctx after arming its deadline, which can overwrite the immediate deadline cancellation sets.
func writeJSON(ctx context.Context, conn net.Conn, value any) error {
	data, err := json.Marshal(value)
	if err != nil {
		return err
	}
	if len(data) > bridgecontract.RelayControlBytes {
		return errors.New("webrelay: control response too large")
	}
	frame := make([]byte, 4+len(data))
	binary.BigEndian.PutUint32(frame, uint32(len(data)))
	copy(frame[4:], data)
	if err := conn.SetWriteDeadline(time.Now().Add(helloTimeout)); err != nil {
		return err
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	for len(frame) > 0 {
		n, err := conn.Write(frame)
		if err != nil {
			return err
		}
		if n == 0 {
			return io.ErrNoProgress
		}
		frame = frame[n:]
	}
	return nil
}

// readHello applies the pre-auth deadline and validates the shared channel contract.
func readHello(conn net.Conn, channel string) (hello, error) {
	var request hello
	if err := conn.SetReadDeadline(time.Now().Add(helloTimeout)); err != nil {
		return request, err
	}
	err := readJSON(conn, &request)
	if err == nil && (request.Version != bridgecontract.RelayVersion || request.Channel != channel ||
		(channel == bridgecontract.RelayChannelControl && (!slices.Contains(request.Channels, bridgecontract.RelayChannelControl) || !slices.Contains(request.Channels, bridgecontract.RelayChannelPlay)))) {
		err = errors.New("webrelay: incompatible Hello")
	}
	if err == nil {
		err = conn.SetReadDeadline(time.Time{})
	}
	return request, err
}

// runControl coalesces account/progress changes and bounds queued errors and silent peers.
func (relay *Relay) runControl(transportCtx context.Context, conn net.Conn, client netip.Addr, controller *sessionController) {
	ctx, cancel := context.WithCancel(controller.ctx)
	defer cancel()
	stopTransport := context.AfterFunc(transportCtx, cancel)
	defer stopTransport()
	if transportCtx.Err() != nil || ctx.Err() != nil {
		return
	}
	session := controller.session
	stop := context.AfterFunc(ctx, func() { _ = conn.SetDeadline(time.Now()) })
	defer stop()
	events := make(chan controlEvent, 4)
	readerDone := make(chan struct{})
	if conn.SetReadDeadline(time.Now().Add(2*keepAliveInterval)) != nil {
		return
	}
	go func() {
		defer close(readerDone)
		defer cancel()
		requests := rate.NewLimiter(rate.Every(time.Duration(bridgecontract.RelayControlRefillMillis)*time.Millisecond), controlRequestBurst)
		for {
			if ctx.Err() != nil {
				return
			}
			var request controlRequest
			if readJSON(conn, &request) != nil || !requests.Allow() {
				return
			}
			var err error
			switch request.Op {
			case bridgecontract.RelayOpSignIn, bridgecontract.RelayOpSignOut:
				err = relay.applyAccount(controller, client, request.Op, time.Now())
			case bridgecontract.RelayOpPong:
				if relay.acknowledge(controller, request.Resume, time.Now()) &&
					conn.SetReadDeadline(time.Now().Add(2*keepAliveInterval)) != nil {
					return
				}
				continue
			default:
				err = errors.New("unknown operation")
			}
			if err != nil {
				select {
				case events <- controlEvent{Type: bridgecontract.RelayEventError, Error: refusal(err)}:
				case <-ctx.Done():
					return
				}
			}
		}
	}()
	defer func() { cancel(); _ = conn.SetDeadline(time.Now()); <-readerDone }()
	welcome, err := relay.welcome(controller, false, time.Now())
	if err != nil || writeJSON(ctx, conn, welcome) != nil {
		return
	}
	state, changed := session.Auth()
	if writeJSON(ctx, conn, controlEvent{Type: bridgecontract.RelayEventAccount, Account: &state}) != nil {
		return
	}
	_, connectChanged := session.Connect()
	keepAlive := time.NewTicker(keepAliveInterval)
	defer keepAlive.Stop()
	for {
		var event controlEvent
		select {
		case <-ctx.Done():
			return
		case <-changed:
			state, changed = session.Auth()
			event = controlEvent{Type: bridgecontract.RelayEventAccount, Account: &state}
		case <-connectChanged:
			// Multiple pack updates become one current observation.
			timer := time.NewTimer(time.Duration(bridgecontract.RelayProgressCoalesceMillis) * time.Millisecond)
			select {
			case <-timer.C:
			case <-ctx.Done():
				timer.Stop()
				return
			}
			progress, next := session.Connect()
			connectChanged = next
			event = controlEvent{Type: bridgecontract.RelayEventConnect, Connect: &progress}
		case event = <-events:
		case <-keepAlive.C:
			var err error
			event, err = relay.welcome(controller, true, time.Now())
			if err != nil {
				return
			}
			event.Type = bridgecontract.RelayEventPing
		}
		if writeJSON(ctx, conn, event) != nil {
			return
		}
	}
}

// applyAccount orders account commands with controller takeover so retired readers cannot mutate it.
func (relay *Relay) applyAccount(controller *sessionController, client netip.Addr, op string, now time.Time) error {
	session := controller.session
	session.commands.Lock()
	defer session.commands.Unlock()
	relay.mu.Lock()
	current := session.controller == controller && controller.ctx.Err() == nil
	allowed := current && (op != bridgecontract.RelayOpSignIn || relay.allowLocked(client, now))
	relay.mu.Unlock()
	if !current {
		return net.ErrClosed
	}
	if !allowed {
		return errAdmission
	}
	if op == bridgecontract.RelayOpSignIn {
		return session.SignIn()
	}
	return session.SignOut()
}

// runPlay claims one generation and preserves the core's reliable bridge byte stream.
func (relay *Relay) runPlay(ctx context.Context, conn net.Conn, client netip.Addr, bound *sessionController, request hello) {
	defer conn.Close()
	session, controlCtx, err := relay.playSession(bound, request, time.Now())
	if err != nil || !relay.allow(client) {
		return
	}
	ctx, cancel := context.WithCancel(ctx)
	defer cancel()
	stopControl := context.AfterFunc(controlCtx, cancel)
	defer stopControl()
	if controlCtx.Err() != nil {
		return
	}
	detach, err := session.attach()
	if err != nil {
		return
	}
	defer func() { detach(time.Now()) }()
	handler, err := session.beginPlay(ctx, controlCtx, request.Join)
	if err != nil {
		return
	}
	defer session.endPlay()
	stop := context.AfterFunc(ctx, func() { _ = conn.SetDeadline(time.Now()) })
	defer stop()
	limited := &uploadLimitedConn{Conn: conn, ctx: ctx, limiter: rate.NewLimiter(bridgecontract.RelayUploadBytesPerSecond, bridgecontract.RelayUploadBurstBytes)}
	// Provider and server errors can contain private addresses; control receives only refusal codes.
	if request.Join == 0 {
		_ = handler.ServeConn(ctx, limited)
		return
	}
	if !session.claimPackJoin(controlCtx, request.Join) {
		return
	}
	delivery := proxy.SessionPackStream{Join: request.Join}
	if transport, ok := conn.(*transportConn); ok {
		delivery.Open = func(ctx context.Context) (io.WriteCloser, error) {
			stream, err := transport.session.OpenUniStreamSync(ctx)
			if err != nil {
				return nil, err
			}
			context.AfterFunc(ctx, func() { stream.CancelWrite(0) })
			return stream, nil
		}
	}
	_ = handler.ServeConnWithPacks(ctx, limited, delivery)
}

// uploadLimitedConn applies upload pacing with the owning transport's cancellation context.
type uploadLimitedConn struct {
	net.Conn
	ctx     context.Context
	limiter *rate.Limiter
}

// Read never returns throttled bytes when cancellation wins the pacing wait.
func (conn *uploadLimitedConn) Read(p []byte) (int, error) {
	if err := conn.ctx.Err(); err != nil {
		return 0, err
	}
	p = p[:min(len(p), conn.limiter.Burst())]
	n, err := conn.Conn.Read(p)
	if n > 0 {
		if waitErr := conn.limiter.WaitN(conn.ctx, n); waitErr != nil {
			return 0, waitErr
		}
	}
	return n, err
}

// refusal reports a bounded public code without account, address, or provider detail.
func refusal(err error) string {
	switch {
	case errors.Is(err, errAdmission):
		return bridgecontract.RelayRefusalAdmission
	case errors.Is(err, errResume):
		return bridgecontract.RelayRefusalResume
	case errors.Is(err, errSessionBusy):
		return bridgecontract.RelayRefusalSessionBusy
	default:
		return bridgecontract.RelayRefusalUnknown
	}
}
