package webrelay

import (
	"context"
	"crypto/tls"
	"errors"
	"io"
	"net"
	"net/http"
	"sync"
	"time"

	"github.com/coder/websocket"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/quic-go/quic-go"
	"github.com/quic-go/quic-go/http3"
	"github.com/quic-go/webtransport-go"
)

// ListenConfig supplies the operator's hostname, UDP/TCP binds and certificate paths.
type ListenConfig struct {
	Hostname        string
	UDPAddress      string
	TCPAddress      string
	CertificateFile string
	KeyFile         string
}

// Serve runs HTTP/3 and HTTPS fallback listeners until cancellation or a listener failure.
func (relay *Relay) Serve(ctx context.Context, cfg ListenConfig) error {
	if cfg.Hostname == "" || cfg.UDPAddress == "" || cfg.TCPAddress == "" {
		return errors.New("webrelay: hostname and both listen addresses are required")
	}
	certificates, err := newCertificateReloader(cfg, time.Now())
	if err != nil {
		return err
	}
	tlsConfig := &tls.Config{MinVersion: tls.VersionTLS13, GetCertificate: certificates.getCertificate}
	wt := relay.transportServer(tlsConfig, cfg.UDPAddress)
	tcp := relay.socketServer(tlsConfig, cfg.TCPAddress)
	tcpListener, err := net.Listen("tcp", cfg.TCPAddress)
	if err != nil {
		return err
	}
	tcpListener = &admittedTCPListener{Listener: tcpListener, relay: relay}
	defer tcpListener.Close()
	udpListener, err := net.ListenPacket("udp", cfg.UDPAddress)
	if err != nil {
		return err
	}
	defer udpListener.Close()
	ctx, cancel := context.WithCancel(ctx)
	defer cancel()
	errs := make(chan error, 2)
	go func() { errs <- tcp.ServeTLS(tcpListener, "", "") }()
	go func() { errs <- relay.serveQUIC(ctx, udpListener, wt) }()
	var result error
	received := 0
	ticker := time.NewTicker(time.Minute)
	defer ticker.Stop()
listening:
	for {
		select {
		case result = <-errs:
			received++
			break listening
		case <-ctx.Done():
			break listening
		case <-relay.ctx.Done():
			break listening
		case now := <-ticker.C:
			if err := certificates.reload(now); err != nil {
				relay.logger.Warn("relay certificate reload failed; retaining previous certificate")
			}
		}
	}
	cancel()
	_ = tcp.Close()
	_ = wt.Close()
	relay.Close()
	// Both listener workers finish before their state can be released by the caller.
	for received < 2 {
		<-errs
		received++
	}
	return result
}

// socketServer limits fallback to HTTP/1, so one counted TCP socket cannot multiplex requests.
func (relay *Relay) socketServer(tlsConfig *tls.Config, address string) *http.Server {
	protocols := new(http.Protocols)
	protocols.SetHTTP1(true)
	return &http.Server{Addr: address, Handler: relay.Handler(), TLSConfig: tlsConfig, Protocols: protocols,
		ReadHeaderTimeout: helloTimeout, ReadTimeout: helloTimeout, WriteTimeout: helloTimeout,
		IdleTimeout: 2 * keepAliveInterval, MaxHeaderBytes: bridgecontract.RelayControlBytes,
		ConnContext: relay.tcpConnContext, ConnState: relay.tcpConnState}
}

// Handler exposes only the two WebSocket fallback channels.
func (relay *Relay) Handler() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("GET "+bridgecontract.RelayControlPath, func(w http.ResponseWriter, r *http.Request) {
		relay.serveSocket(w, r, bridgecontract.RelayChannelControl)
	})
	mux.HandleFunc("GET "+bridgecontract.RelayPlayPath, func(w http.ResponseWriter, r *http.Request) { relay.serveSocket(w, r, bridgecontract.RelayChannelPlay) })
	return mux
}

// serveSocket authenticates inside binary application framing, never URL or subprotocol metadata.
func (relay *Relay) serveSocket(w http.ResponseWriter, r *http.Request, channel string) {
	if !relay.checkOrigin(r) {
		http.Error(w, "origin refused", http.StatusForbidden)
		return
	}
	client := clientAddr(r)
	release, err := relay.admitSocket(r)
	if err != nil {
		http.Error(w, refusal(err), http.StatusTooManyRequests)
		return
	}
	defer release()
	socket, err := websocket.Accept(w, r, &websocket.AcceptOptions{Subprotocols: []string{bridgecontract.RelaySocketProtocol}, InsecureSkipVerify: true})
	if err != nil {
		return
	}
	defer socket.CloseNow()
	if socket.Subprotocol() != bridgecontract.RelaySocketProtocol {
		return
	}
	ctx, cancel := context.WithCancel(relay.ctx)
	defer cancel()
	stop := context.AfterFunc(ctx, func() { _ = socket.CloseNow() })
	defer stop()
	limit := playMessageLimit
	if channel == bridgecontract.RelayChannelControl {
		limit = bridgecontract.RelayControlBytes + 4
	}
	conn := newSocketConn(ctx, socket, limit)
	request, err := readHello(conn, channel)
	if err != nil {
		return
	}
	if channel == bridgecontract.RelayChannelPlay {
		relay.runPlay(ctx, conn, client, nil, request)
		return
	}
	controller, detach, err := relay.openControl(client, request.Resume, time.Now())
	if err != nil {
		_ = writeJSON(ctx, conn, controlEvent{Type: bridgecontract.RelayEventError, Error: refusal(err)})
		return
	}
	defer func() { detach(time.Now()) }()
	stopController := context.AfterFunc(controller.ctx, cancel)
	defer stopController()
	relay.runControl(ctx, conn, client, controller)
}

// newSocketConn preserves the reliable byte stream while bounding incoming and outgoing messages.
// The message limit must be positive.
func newSocketConn(ctx context.Context, socket *websocket.Conn, limit int) net.Conn {
	conn := websocket.NetConn(ctx, socket, websocket.MessageBinary)
	socket.SetReadLimit(int64(limit)) // NetConn removes the library's message limit.
	return &socketConn{Conn: conn, messageLimit: limit}
}

// socketConn keeps each complete Write ordered across its bounded WebSocket messages.
type socketConn struct {
	net.Conn
	messageLimit int
	writeMu      sync.Mutex
}

// Write splits the stream into bounded messages and reports bytes accepted before any failure.
func (conn *socketConn) Write(data []byte) (int, error) {
	conn.writeMu.Lock()
	defer conn.writeMu.Unlock()
	written := 0
	for len(data) > 0 {
		chunk := data[:min(len(data), conn.messageLimit)]
		n, err := conn.Conn.Write(chunk)
		written += n
		if err != nil {
			return written, err
		}
		if n != len(chunk) {
			return written, io.ErrShortWrite
		}
		data = data[n:]
	}
	return written, nil
}

// serveTransport binds one control and at most one play stream to an authenticated QUIC session.
func (relay *Relay) serveTransport(w http.ResponseWriter, r *http.Request, server *webtransport.Server) {
	client := clientAddr(r)
	admission, release, err := relay.admitTransport(r)
	if err != nil {
		http.Error(w, refusal(err), http.StatusTooManyRequests)
		return
	}
	defer release()
	defer admission.conn.CloseWithError(0, "")
	transport, err := server.Upgrade(w, r)
	if err != nil {
		return
	}
	defer transport.CloseWithError(0, "")
	ctx, cancel := context.WithCancel(relay.ctx)
	defer cancel()
	stop := context.AfterFunc(transport.Context(), cancel)
	defer stop()
	stopClose := context.AfterFunc(ctx, func() { _ = transport.CloseWithError(0, "") })
	defer stopClose()
	authCtx, authCancel := context.WithTimeout(ctx, helloTimeout)
	stream, err := transport.AcceptStream(authCtx)
	authCancel()
	if err != nil {
		return
	}
	controlConn := &transportConn{Stream: stream, session: transport}
	request, err := readHello(controlConn, bridgecontract.RelayChannelControl)
	if err != nil {
		return
	}
	controller, detach, err := relay.openControl(client, request.Resume, time.Now())
	if err != nil {
		_ = writeJSON(ctx, controlConn, controlEvent{Type: bridgecontract.RelayEventError, Error: refusal(err)})
		return
	}
	defer func() { detach(time.Now()) }()
	stopController := context.AfterFunc(controller.ctx, cancel)
	defer stopController()
	if !admission.establish() {
		return
	}
	var workers sync.WaitGroup
	defer func() { cancel(); _ = transport.CloseWithError(0, ""); workers.Wait() }()
	workers.Go(func() { relay.runControl(ctx, controlConn, client, controller); cancel() })
	// Older peers may not negotiate session stream limits. Reject uni streams explicitly too.
	workers.Go(func() {
		for {
			stream, err := transport.AcceptUniStream(ctx)
			if err != nil {
				return
			}
			stream.CancelRead(0)
		}
	})
	for {
		stream, err := transport.AcceptStream(ctx)
		if err != nil {
			return
		}
		conn := &transportConn{Stream: stream, session: transport}
		request, err := readHello(conn, bridgecontract.RelayChannelPlay)
		if err != nil {
			conn.abort()
			continue
		}
		// Leave replacement streams in QUIC's bounded accept queue until teardown completes.
		relay.runPlay(ctx, conn, client, controller, request)
	}
}

// transportConn retains QUIC addresses and deadlines while presenting the core's net.Conn boundary.
type transportConn struct {
	*webtransport.Stream
	session *webtransport.Session
}

// LocalAddr returns the QUIC connection's current local address.
func (conn *transportConn) LocalAddr() net.Addr { return conn.session.LocalAddr() }

// RemoteAddr returns the QUIC connection's current peer address.
func (conn *transportConn) RemoteAddr() net.Addr { return conn.session.RemoteAddr() }

// Close ends reads and sends FIN after already accepted terminal frames.
func (conn *transportConn) Close() error { conn.CancelRead(0); return conn.Stream.Close() }

// abort resets both stream directions when an extra or malformed channel is refused.
func (conn *transportConn) abort() { conn.CancelRead(0); conn.CancelWrite(0) }

// transportServer composes the pinned HTTP/3 settings and the Extended CONNECT handler.
func (relay *Relay) transportServer(tlsConfig *tls.Config, address string) *webtransport.Server {
	mux := http.NewServeMux()
	wt := &webtransport.Server{H3: &http3.Server{Addr: address, TLSConfig: http3.ConfigureTLSConfig(tlsConfig.Clone()),
		Handler: mux, MaxHeaderBytes: bridgecontract.RelayControlBytes, QUICConfig: &quic.Config{
			MaxIncomingStreams: 8, MaxIncomingUniStreams: 4, MaxStreamReceiveWindow: playMessageLimit, MaxConnectionReceiveWindow: 2 * playMessageLimit,
			HandshakeIdleTimeout: helloTimeout, MaxIdleTimeout: 2 * keepAliveInterval}},
		Config: &webtransport.Config{MaxIncomingStreams: bridgecontract.RelayBidirectionalStreams, MaxIncomingUniStreams: -1, MaxIncomingData: 2 * playMessageLimit}, CheckOrigin: relay.checkOrigin}
	mux.HandleFunc(bridgecontract.RelayTransportPath, func(w http.ResponseWriter, r *http.Request) { relay.serveTransport(w, r, wt) })
	return wt
}
