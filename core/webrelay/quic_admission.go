package webrelay

import (
	"context"
	"net"
	"net/http"
	"net/netip"
	"sync"
	"time"

	"github.com/quic-go/quic-go"
	"github.com/quic-go/webtransport-go"
)

// quicAdmissionKey carries handshake admission into HTTP/3 requests.
type quicAdmissionKey struct{}

// admittedQUICConn shares one slot with its HTTP/3 worker and sole WebTransport session.
type admittedQUICConn struct {
	*connectionAdmission
	relay       *Relay
	conn        *quic.Conn
	deadline    time.Time
	claimed     bool
	ready       bool
	cancelHello context.CancelFunc
}

// quicConnContext admits peers before QUIC allocates a connection or starts TLS.
func (relay *Relay) quicConnContext(ctx context.Context, info *quic.ClientInfo) (context.Context, error) {
	address, err := netip.ParseAddrPort(info.RemoteAddr.String())
	if err != nil {
		return nil, errAdmission
	}
	now := time.Now()
	release, err := relay.admit(address.Addr().Unmap(), now)
	if err != nil {
		return nil, err
	}
	admission := &admittedQUICConn{connectionAdmission: newConnectionAdmission(release), relay: relay, deadline: now.Add(helloTimeout)}
	context.AfterFunc(ctx, admission.close)
	return context.WithValue(ctx, quicAdmissionKey{}, admission), nil
}

// serveQUIC owns the UDP transport, including handshakes and connections with no HTTP requests.
func (relay *Relay) serveQUIC(ctx context.Context, socket net.PacketConn, server *webtransport.Server) error {
	ctx, cancel := context.WithCancel(ctx)
	defer cancel()
	stopRelay := context.AfterFunc(relay.ctx, cancel)
	defer stopRelay()
	transport := &quic.Transport{Conn: socket, ConnContext: relay.quicConnContext}
	closeTransport := sync.OnceFunc(func() { _ = transport.Close() })
	defer closeTransport()
	config := server.H3.QUICConfig.Clone()
	config.EnableDatagrams = true
	config.EnableStreamResetPartialDelivery = true
	listener, err := transport.ListenEarly(server.H3.TLSConfig, config)
	if err != nil {
		return err
	}
	stop := context.AfterFunc(ctx, closeTransport)
	defer stop()
	var workers sync.WaitGroup
	defer func() {
		_ = listener.Close()
		closeTransport()
		_ = server.Close()
		workers.Wait()
	}()
	for {
		conn, err := listener.Accept(ctx)
		if err != nil {
			return err
		}
		admission, ok := conn.Context().Value(quicAdmissionKey{}).(*admittedQUICConn)
		if !ok || admission.relay != relay {
			_ = conn.CloseWithError(0, "")
			continue
		}
		finish, err := admission.retain()
		if err != nil {
			_ = conn.CloseWithError(0, "")
			continue
		}
		admission.conn = conn
		workers.Go(func() {
			defer finish()
			defer conn.CloseWithError(0, "")
			admission.guardHello(conn)
			_ = server.ServeQUICConn(conn)
		})
	}
}

// guardHello bounds all initial HTTP/3, stream-prefix and Hello parsing with one deadline.
func (admission *admittedQUICConn) guardHello(conn *quic.Conn) {
	ctx, cancel := context.WithDeadline(conn.Context(), admission.deadline)
	admission.mu.Lock()
	admission.cancelHello = cancel
	admission.mu.Unlock()
	context.AfterFunc(ctx, func() {
		admission.mu.Lock()
		ready := admission.ready
		admission.mu.Unlock()
		if !ready {
			_ = conn.CloseWithError(0, "")
		}
	})
}

// establish ends the initial deadline only after the control Hello has claimed an account.
func (admission *admittedQUICConn) establish() bool {
	admission.mu.Lock()
	if admission.closed || !time.Now().Before(admission.deadline) {
		admission.mu.Unlock()
		return false
	}
	admission.ready = true
	cancel := admission.cancelHello
	admission.mu.Unlock()
	if cancel != nil {
		cancel()
	}
	return true
}

// admitTransport allows one session per counted QUIC connection, retaining its handler.
func (relay *Relay) admitTransport(request *http.Request) (*admittedQUICConn, func(), error) {
	admission, ok := request.Context().Value(quicAdmissionKey{}).(*admittedQUICConn)
	if !ok || admission.relay != relay {
		return nil, nil, errAdmission
	}
	admission.mu.Lock()
	defer admission.mu.Unlock()
	if admission.closed || admission.claimed || request.Context().Err() != nil {
		return nil, nil, errAdmission
	}
	admission.claimed = true
	admission.refs++
	return admission, sync.OnceFunc(admission.drop), nil
}
