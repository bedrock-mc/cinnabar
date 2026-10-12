package proxy

import (
	"context"
	"errors"
	"log/slog"
	"net"
	"net/http"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/sandertv/go-raknet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/device"
)

// SessionHandlerConfig configures sessions served over connections the caller accepts, such as a web
// relay's, instead of the core's own session endpoint.
type SessionHandlerConfig struct {
	Account *authcache.Account // nil joins offline
	Logger  *slog.Logger
	// Dialer opens every upstream socket: RakNet joins, transfer hops included, and HTTP requests
	// such as URL pack downloads, so the dialer decides which hosts the handler may reach.
	Dialer raknet.UpstreamDialer
	// SelectTarget maps a Connect's connect.v1 target to an address; nil rejects targeted Connects.
	SelectTarget func(ctx context.Context, kind, value string) (string, error)
	Device       *device.Profile // nil keeps the client's claimed device
	OnDisconnect func(DisconnectInfo)
	// ConnectProgress receives the join's live stage and pack download progress; a zero stage
	// ends it. Callbacks must return promptly.
	ConnectProgress func(ConnectProgress)
}

// SessionHandler serves one account's sessions. A targeted Connect replaces any pending transfer;
// a targetless Connect joins the transfer the previous session recorded.
type SessionHandler struct {
	server *sessionServer
}

// NewSessionHandler returns a handler whose upstream joins dial through cfg.Dialer only.
func NewSessionHandler(cfg SessionHandlerConfig) (*SessionHandler, error) {
	if cfg.Dialer == nil {
		return nil, errors.New("proxy: session handler dialer is required")
	}
	network := minecraft.RakNet{MaxMTU: remoteMaxMTU, UpstreamDialer: cfg.Dialer}
	logger := cfg.Logger
	if logger == nil {
		logger = slog.Default()
	}
	transfers := new(TransferState)
	prepared := newPreparedConnections("", cfg.Account, logger)
	prepared.addressNetwork = network
	prepared.httpClient = dialerHTTPClient(cfg.Dialer)
	prepared.connectProgress = cfg.ConnectProgress
	dial := func(_ context.Context, address string) (*resolvedUpstreamTarget, error) {
		return &resolvedUpstreamTarget{address: address, network: network}, nil
	}
	prepared.dialTarget = consumeTransferOnDial(prepared.dialTarget, transfers)
	prepared.resolveTarget = withPendingTransfer(transfers, dial, func(context.Context) (*resolvedUpstreamTarget, error) {
		return nil, errors.New("proxy: session has no target")
	})
	var selectTarget func(ctx context.Context, kind, value string) (string, error)
	if cfg.SelectTarget != nil {
		selectTarget = func(ctx context.Context, kind, value string) (string, error) {
			address, err := cfg.SelectTarget(ctx, kind, value)
			if err == nil {
				transfers.Clear()
			}
			return address, err
		}
	}
	return &SessionHandler{server: &sessionServer{
		prepared:     prepared,
		transfers:    transfers,
		onDisconnect: cfg.OnDisconnect,
		selectTarget: selectTarget,
		dialTarget:   dial,
		device:       cfg.Device,
		logger:       logger,
	}}, nil
}

// dialerHTTPClient returns a client whose every connection, redirect hops included, is opened by
// dialer; it ignores environment proxies, which would carry requests past the dialer.
func dialerHTTPClient(dialer raknet.UpstreamDialer) *http.Client {
	return &http.Client{Transport: &http.Transport{
		DialContext:         dialer.DialContext,
		ForceAttemptHTTP2:   true,
		TLSHandshakeTimeout: 10 * time.Second,
		IdleConnTimeout:     90 * time.Second,
	}}
}

// ServeConn runs one session on conn until it ends; the caller closes conn afterwards.
func (handler *SessionHandler) ServeConn(ctx context.Context, conn net.Conn) error {
	return handler.server.serveConn(ctx, conn)
}

// Close ends upstream preparations still in flight; callers first end every ServeConn.
func (handler *SessionHandler) Close() error {
	handler.server.prepared.httpClient.CloseIdleConnections()
	return handler.server.prepared.shutdown()
}
