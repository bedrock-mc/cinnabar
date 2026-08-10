// Package proxy joins a local gophertunnel listener session to an upstream
// Bedrock server and relays decoded packet values between them.
package proxy

import (
	"context"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net"
	"strconv"
	"strings"
	"sync"

	"github.com/google/uuid"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"github.com/sandertv/gophertunnel/minecraft/resource"
	"golang.org/x/oauth2"
)

// Config configures a local bridge listener and its upstream Bedrock server.
type Config struct {
	SocketDir   string
	Upstream    string
	TokenSource oauth2.TokenSource
	Logger      *slog.Logger
	// ResourcePackCache is an optional process-owned cache. Serve never closes it.
	ResourcePackCache minecraft.ResourcePackCache
	// ResourcePackAdmission receives one secret-safe final snapshot per upstream
	// preparation attempt. Callbacks must return promptly.
	ResourcePackAdmission func(ResourcePackAdmissionSnapshot)
}

const localRelayBatchPacketLimit = 1600
const maxInitialTransferHops = 8

type acceptResult struct {
	conn net.Conn
	err  error
}

type connectionAcceptor interface {
	Accept() (net.Conn, error)
}

// Serve listens for local bridge clients until ctx is cancelled. Session
// setup failures are returned; ordinary peer disconnects leave the listener
// available for another client.
func Serve(ctx context.Context, cfg Config) (err error) {
	logger := cfg.Logger
	if logger == nil {
		logger = slog.Default()
	}
	if cfg.SocketDir == "" {
		return errors.New("proxy: socket directory is required")
	}
	if cfg.Upstream == "" {
		return errors.New("proxy: upstream address is required")
	}
	serveCtx, cancel := context.WithCancel(ctx)
	defer cancel()
	sessionErr := make(chan error, 1)
	prepared := newPreparedConnections(cfg.Upstream, cfg.TokenSource, logger)
	prepared.resourcePackCache = cfg.ResourcePackCache
	prepared.resourcePackAdmission = cfg.ResourcePackAdmission
	listener, err := (minecraft.ListenConfig{
		AuthenticationDisabled: true,
		AllowUnknownPackets:    true,
		EnableBatchReading:     true,
		ErrorLog:               slog.Default().With("component", "local-listener"),
		PrepareResourcePackOffer: func(ctx context.Context, conn *minecraft.Conn) error {
			prepareErr := prepared.prepare(ctx, conn)
			reportPreparationError(sessionErr, prepareErr, serveCtx)
			return prepareErr
		},
	}).ListenNetwork(streamnet.New(cfg.SocketDir), "")
	if err != nil {
		return errors.Join(fmt.Errorf("proxy: listen: %w", err), prepared.shutdown())
	}
	reportListenerReady(logger, cfg.SocketDir)

	accepted := make(chan acceptResult)
	acceptDone := make(chan error, 1)
	go func() {
		acceptDone <- runAcceptLoop(serveCtx, listener, accepted)
	}()

	var sessions sync.WaitGroup
	var stopOnce sync.Once
	var stopErr error
	stop := func() error {
		stopOnce.Do(func() {
			stopErr = stopServer(cancel, listener, &sessions, acceptDone)
		})
		return stopErr
	}
	defer func() { err = errors.Join(err, shutdownPreparedServer(prepared, stop)) }()
	for {
		select {
		case <-ctx.Done():
			return nil
		case result := <-accepted:
			if result.err != nil {
				if serveCtx.Err() != nil || errors.Is(result.err, net.ErrClosed) {
					return nil
				}
				return fmt.Errorf("proxy: accept: %w", result.err)
			}
			downstream, ok := result.conn.(*minecraft.Conn)
			if !ok {
				cleanupErr := cleanupHandoffConnection(result.conn)
				return errors.Join(fmt.Errorf("proxy: accepted unexpected connection type %T", result.conn), cleanupErr)
			}
			upstream, handoffErr := takePreparedAfterAccept(prepared, downstream)
			if handoffErr != nil {
				return handoffErr
			}
			if upstream == nil {
				continue
			}
			sessions.Add(1)
			go func() {
				defer sessions.Done()
				err := serveAcceptedConnection(serveCtx, downstream, upstream, cfg.SocketDir, logger)
				if err != nil && !isOrdinaryClose(err) {
					select {
					case sessionErr <- err:
					default:
					}
				}
			}()
		case err := <-sessionErr:
			return err
		}
	}
}

type acceptedDownstreamSession interface {
	downstreamSession
	ClientCacheEnabled() bool
}

func serveAcceptedConnection(
	ctx context.Context,
	downstream acceptedDownstreamSession,
	prepared *preparedConnection,
	socketDir string,
	logger *slog.Logger,
) (err error) {
	serveStarted := false
	defer func() {
		if recovered := recover(); recovered != nil {
			err = errors.Join(err, panicTypeError("starting prepared downstream session", recovered))
		}
		if !serveStarted {
			err = errors.Join(err, shutdownSession(downstream), prepared.close())
		}
	}()
	reportLocalClientAccepted(logger, socketDir, downstream.ClientCacheEnabled())
	serveStarted = true
	return servePreparedConnection(ctx, downstream, prepared)
}

func shutdownPreparedServer(prepared *preparedConnections, stop func() error) error {
	prepared.beginShutdown()
	stopErr := stop()
	return errors.Join(stopErr, prepared.finishShutdown())
}

func takePreparedAfterAccept(prepared *preparedConnections, downstream *minecraft.Conn) (*preparedConnection, error) {
	upstream, ok := prepared.take(downstream)
	if ok {
		return upstream, nil
	}
	peerErr := downstream.Context().Err()
	cleanupErr := cleanupHandoffConnection(downstream)
	if peerErr != nil {
		return nil, nil
	}
	return nil, errors.Join(errors.New("proxy: accepted connection has no prepared upstream"), cleanupErr)
}

func shouldSurfacePreparationError(err error, serveCtx context.Context) bool {
	if err == nil || serveCtx.Err() != nil {
		return false
	}
	var admissionErr *PackAdmissionError
	if errors.As(err, &admissionErr) {
		return false
	}
	var cancellationErr *preparationCancellationError
	return !errors.As(err, &cancellationErr)
}

func reportPreparationError(sessionErr chan<- error, err error, serveCtx context.Context) {
	if !shouldSurfacePreparationError(err, serveCtx) {
		return
	}
	select {
	case sessionErr <- fmt.Errorf("proxy: prepare upstream: %w", err):
	default:
	}
}

func runAcceptLoop(ctx context.Context, listener connectionAcceptor, accepted chan<- acceptResult) (err error) {
	defer func() {
		if recovered := recover(); recovered != nil {
			err = fmt.Errorf("panic in accept loop: %v", recovered)
		}
	}()
	for {
		conn, acceptErr := listener.Accept()
		if acceptErr != nil && conn != nil {
			acceptErr = errors.Join(acceptErr, cleanupHandoffConnection(conn))
			conn = nil
		}
		select {
		case accepted <- acceptResult{conn: conn, err: acceptErr}:
		case <-ctx.Done():
			return cleanupHandoffConnection(conn)
		}
		if acceptErr != nil {
			return nil
		}
	}
}

func stopServer(cancel context.CancelFunc, listener io.Closer, sessions *sync.WaitGroup, acceptDone <-chan error) error {
	cancel()
	closeErr := listener.Close()
	acceptErr := <-acceptDone
	sessions.Wait()
	return errors.Join(closeErr, acceptErr)
}

func cleanupHandoffConnection(conn net.Conn) error {
	if conn == nil {
		return nil
	}
	var abortErr error
	if abortable, ok := conn.(interface{ Abort() error }); ok {
		abortErr = callConnectionLifecycle("aborting", abortable.Abort)
	}
	return errors.Join(abortErr, callConnectionLifecycle("closing", conn.Close))
}

func callConnectionLifecycle(operation string, call func() error) (err error) {
	defer func() {
		if recovered := recover(); recovered != nil {
			err = fmt.Errorf("panic while %s accepted connection: %v", operation, recovered)
		}
	}()
	return call()
}

func reportLocalClientAccepted(logger *slog.Logger, socketDir string, clientCacheEnabled bool) {
	logger.Info(
		"local client accepted",
		"socket_dir", socketDir,
		"client_blob_cache", clientCacheEnabled,
	)
}

func reportListenerReady(logger *slog.Logger, socketDir string) {
	attributes := []any{"socket_dir", socketDir}
	if network, endpoint, err := streamnet.Resolve(socketDir); err == nil {
		attributes = append(attributes, "network", network, "endpoint", endpoint)
	}
	logger.Info("listener ready; waiting for local Rust client", attributes...)
}

func authenticationMode(tokenSource oauth2.TokenSource) string {
	if tokenSource == nil {
		return "offline"
	}
	return "microsoft"
}

func connectUpstream(
	ctx context.Context,
	address string,
	authentication string,
	logger *slog.Logger,
	dial func(context.Context, string) (upstreamSession, error),
) (result upstreamSession, err error) {
	var owned upstreamSession
	defer func() {
		if recovered := recover(); recovered != nil {
			err = errors.Join(err, panicTypeError("reporting upstream connection status", recovered))
			result = nil
		}
		if result == nil && owned != nil {
			err = errors.Join(err, finishPreparedResources(true, owned, nil, nil, nil))
		}
	}()
	logger.Info("upstream connection starting", "target", address, "authentication", authentication)
	upstream, err := dialFollowingTransfers(ctx, address, func(ctx context.Context, address string) (upstreamSession, error) {
		upstream, dialErr := dial(ctx, address)
		if dialErr != nil && upstream != nil {
			dialErr = errors.Join(dialErr, finishPreparedResources(true, upstream, nil, nil, nil))
			upstream = nil
		}
		return upstream, dialErr
	})
	if err != nil {
		logger.Error("upstream connection failed", "target", address, "authentication", authentication, "error", err)
		return nil, err
	}
	owned = upstream
	logger.Info("upstream connected", "target", address, "authentication", authentication)
	result = upstream
	owned = nil
	return result, nil
}

func dialFollowingTransfers(
	ctx context.Context,
	initialAddress string,
	dial func(context.Context, string) (upstreamSession, error),
) (upstreamSession, error) {
	address := initialAddress
	seen := map[string]struct{}{strings.ToLower(address): {}}
	for transfers := 0; ; transfers++ {
		upstream, err := dial(ctx, address)
		if err == nil {
			return upstream, nil
		}
		var transfer *minecraft.TransferError
		if !errors.As(err, &transfer) {
			return nil, err
		}
		if transfers >= maxInitialTransferHops {
			return nil, fmt.Errorf("proxy: too many transfers before login (limit %d): %w", maxInitialTransferHops, err)
		}
		next, targetErr := initialTransferTarget(transfer)
		if targetErr != nil {
			return nil, errors.Join(targetErr, err)
		}
		key := strings.ToLower(next)
		if _, ok := seen[key]; ok {
			return nil, fmt.Errorf("proxy: transfer cycle to %q: %w", next, err)
		}
		slog.Info("following pre-login server transfer", "from", address, "to", next, "hop", transfers+1)
		seen[key] = struct{}{}
		address = next
	}
}

func initialTransferTarget(transfer *minecraft.TransferError) (string, error) {
	if transfer == nil {
		return "", errors.New("proxy: invalid transfer: nil transfer")
	}
	host := strings.TrimSpace(transfer.Address)
	if host == "" {
		return "", errors.New("proxy: invalid transfer: empty address")
	}
	if transfer.Port == 0 {
		return "", errors.New("proxy: invalid transfer: zero port")
	}
	if strings.HasPrefix(host, "[") && strings.HasSuffix(host, "]") {
		host = strings.TrimSpace(host[1 : len(host)-1])
		if host == "" {
			return "", errors.New("proxy: invalid transfer: empty address")
		}
	}
	return net.JoinHostPort(host, strconv.Itoa(int(transfer.Port))), nil
}

type dialerDownstream interface {
	IdentityData() login.IdentityData
	ClientData() login.ClientData
	Proto() minecraft.Protocol
}

func newUpstreamDialer(downstream dialerDownstream, tokenSource oauth2.TokenSource) minecraft.Dialer {
	return newUpstreamDialerWithCacheTelemetry(downstream, tokenSource, nil)
}

func newUpstreamDialerWithCacheTelemetry(
	downstream dialerDownstream,
	tokenSource oauth2.TokenSource,
	cacheTelemetry *cacheBoundaryTelemetry,
) minecraft.Dialer {
	return newUpstreamDialerForAdmission(downstream, tokenSource, cacheTelemetry, nil, nil)
}

func newUpstreamDialerForAdmission(
	downstream dialerDownstream,
	tokenSource oauth2.TokenSource,
	cacheTelemetry *cacheBoundaryTelemetry,
	resourcePackCache minecraft.ResourcePackCache,
	packAdmission *resourcePackAdmissionTelemetry,
) minecraft.Dialer {
	dialer := minecraft.Dialer{
		ClientData:           downstream.ClientData(),
		DownloadResourcePack: acceptResourcePack,
		ResourcePackDownload: boundedResourcePackDownload(),
		EnableBatchReading:   true,
		// ClientCacheStatus is sent after the Listener preparation hook, so the
		// downstream capability is not available at this boundary. Keep the
		// upstream cache disabled until a later, explicit capability seam exists.
		EnableClientCache: false,
		ErrorLog:          secretSafeResourcePackLogger(),
		Protocol:          downstream.Proto(),
		TokenSource:       tokenSource,
		ResourcePackCache: resourcePackCache,
	}
	if cacheTelemetry != nil || packAdmission != nil {
		dialer.PacketFunc = func(header packet.Header, payload []byte, source, destination net.Addr) {
			if cacheTelemetry != nil {
				cacheTelemetry.observeUpstreamPacket(header, payload, source, destination)
			}
			if packAdmission != nil && header.PacketID == packet.IDResourcePacksInfo {
				packAdmission.observeNegotiation()
			}
		}
	}
	if tokenSource == nil {
		identity := downstream.IdentityData()
		dialer.IdentityData = login.IdentityData{
			Identity:    identity.Identity,
			DisplayName: identity.DisplayName,
		}
	}
	return dialer
}

func acceptResourcePack(_ uuid.UUID, _ string, _, _ int) bool { return true }

func boundedResourcePackDownload() minecraft.ResourcePackDownloadConfig {
	return minecraft.ResourcePackDownloadConfig{
		MaxInFlightChunks:  minecraft.DefaultResourcePackMaxInFlightChunks,
		MaxPacks:           minecraft.DefaultResourcePackMaxPacks,
		MaxPackBytes:       minecraft.DefaultResourcePackMaxPackBytes,
		MaxTotalBytes:      minecraft.DefaultResourcePackMaxTotalBytes,
		MaxChunkBytes:      minecraft.DefaultResourcePackMaxChunkBytes,
		MaxChunks:          minecraft.DefaultResourcePackMaxChunks,
		ResponseTimeout:    minecraft.DefaultResourcePackResponseTimeout,
		AllowHTTPDownloads: false,
	}
}

func dialAndServe(ctx context.Context, downstream downstreamSession, dial func(context.Context) (upstreamSession, error)) error {
	return dialAndServeWithCacheTelemetry(ctx, downstream, dial, nil)
}

func dialAndServeWithCacheTelemetry(
	ctx context.Context,
	downstream downstreamSession,
	dial func(context.Context) (upstreamSession, error),
	cacheTelemetry *cacheBoundaryTelemetry,
) error {
	type result struct {
		upstream upstreamSession
		err      error
	}
	results := make(chan result, 1)
	go func() {
		var upstream upstreamSession
		err := callWithoutPanic(func() (err error) {
			upstream, err = dial(ctx)
			return err
		})
		if ctx.Err() != nil && upstream != nil {
			err = errors.Join(err, shutdownSession(upstream))
			upstream = nil
		}
		results <- result{upstream: upstream, err: err}
	}()

	select {
	case <-ctx.Done():
		return errors.Join(ctx.Err(), shutdownSession(downstream))
	case result := <-results:
		if result.err != nil {
			return finishDialFailure(downstream, result.err)
		}
		return serveConnectionsWithCacheTelemetry(ctx, downstream, result.upstream, cacheTelemetry)
	}
}

func finishDialFailure(downstream packetSession, dialErr error) error {
	return errors.Join(fmt.Errorf("proxy: dial upstream: %w", dialErr), shutdownSession(downstream))
}

type packetSession interface {
	ReadBatch() ([]packet.Packet, error)
	WritePacketImmediate(...packet.Packet) error
	Flush() error
	Abort() error
	Close() error
}

type downstreamSession interface {
	packetSession
	StartGameContext(context.Context, minecraft.GameData) error
}

type upstreamSession interface {
	packetSession
	DoSpawnContext(context.Context) error
	GameData() minecraft.GameData
	ResourcePacks() []*resource.Pack
	TexturePacksRequired() bool
}

func serveConnections(ctx context.Context, downstream downstreamSession, upstream upstreamSession) (err error) {
	return serveConnectionsWithCacheTelemetry(ctx, downstream, upstream, nil)
}

func serveConnectionsWithCacheTelemetry(
	ctx context.Context,
	downstream downstreamSession,
	upstream upstreamSession,
	cacheTelemetry *cacheBoundaryTelemetry,
) (err error) {
	defer func() {
		err = errors.Join(err, shutdownSession(downstream), shutdownSession(upstream))
	}()

	if err := spawnBarrier(ctx, downstream, upstream); err != nil {
		return err
	}
	return relayPacketsWithCacheTelemetry(ctx, downstream, upstream, cacheTelemetry)
}

func spawnBarrier(ctx context.Context, downstream downstreamSession, upstream upstreamSession) error {
	barrierCtx, cancel := context.WithCancel(ctx)
	defer cancel()

	type result struct {
		operation string
		err       error
	}
	results := make(chan result, 2)
	go func() {
		results <- result{
			operation: "start downstream game",
			err:       callWithoutPanic(func() error { return downstream.StartGameContext(barrierCtx, upstream.GameData()) }),
		}
	}()
	go func() {
		results <- result{
			operation: "spawn upstream client",
			err:       callWithoutPanic(func() error { return upstream.DoSpawnContext(barrierCtx) }),
		}
	}()

	first := <-results
	if first.err != nil {
		cancel()
	}
	second := <-results
	if second.err != nil {
		cancel()
	}

	var joined error
	for _, result := range []result{first, second} {
		if result.err == nil {
			continue
		}
		if errors.Is(result.err, context.Canceled) && (first.err != nil || second.err != nil) && ctx.Err() == nil {
			continue
		}
		joined = errors.Join(joined, fmt.Errorf("proxy: %s: %w", result.operation, result.err))
	}
	if joined != nil {
		return joined
	}
	return ctx.Err()
}

func relayPackets(ctx context.Context, downstream, upstream packetSession) error {
	return relayPacketsWithCacheTelemetry(ctx, downstream, upstream, nil)
}

func relayPacketsWithCacheTelemetry(
	ctx context.Context,
	downstream, upstream packetSession,
	cacheTelemetry *cacheBoundaryTelemetry,
) error {
	type result struct {
		direction string
		err       error
	}
	results := make(chan result, 2)
	go func() {
		results <- result{"downstream to upstream", pumpPacketsWithCacheTelemetry(downstream, upstream, true, cacheTelemetry)}
	}()
	go func() {
		results <- result{"upstream to downstream", pumpPacketsWithCacheTelemetry(upstream, downstream, false, cacheTelemetry)}
	}()

	var first result
	select {
	case first = <-results:
	case <-ctx.Done():
		first = result{direction: "relay context", err: ctx.Err()}
	}
	closeErr := errors.Join(shutdownSession(downstream), shutdownSession(upstream))

	var second result
	if first.direction == "relay context" {
		one := <-results
		two := <-results
		second = result{direction: one.direction + " and " + two.direction, err: errors.Join(one.err, two.err)}
	} else {
		second = <-results
	}

	if ctx.Err() != nil {
		return errors.Join(ctx.Err(), closeErr)
	}
	for _, result := range []result{first, second} {
		if result.err != nil && !isOrdinaryClose(result.err) {
			return errors.Join(fmt.Errorf("proxy: relay %s: %w", result.direction, result.err), closeErr)
		}
	}
	return closeErr
}

func closeSession(session packetSession) (err error) {
	defer func() {
		if recovered := recover(); recovered != nil {
			err = fmt.Errorf("panic while closing session: %v", recovered)
		}
	}()
	return session.Close()
}

func abortSession(session packetSession) (err error) {
	defer func() {
		if recovered := recover(); recovered != nil {
			err = fmt.Errorf("panic while aborting session: %v", recovered)
		}
	}()
	return session.Abort()
}

func shutdownSession(session packetSession) error {
	return errors.Join(abortSession(session), closeSession(session))
}

func pumpPackets(source, destination packetSession, fromDownstream bool) (err error) {
	return pumpPacketsWithCacheTelemetry(source, destination, fromDownstream, nil)
}

func pumpPacketsWithCacheTelemetry(
	source, destination packetSession,
	fromDownstream bool,
	cacheTelemetry *cacheBoundaryTelemetry,
) (err error) {
	defer func() {
		if recovered := recover(); recovered != nil {
			err = fmt.Errorf("panic while relaying packets: %v", recovered)
		}
	}()
	dropInitialSpawnLoadingScreens := fromDownstream
	var upstreamIdentity login.IdentityData
	if fromDownstream {
		// The Rust client connects to this listener with authentication disabled;
		// its login identity is only a local transport identity. The authenticated
		// upstream Conn is the single canonical account identity that the remote
		// server validates chat against.
		if identitySession, ok := destination.(interface {
			IdentityData() login.IdentityData
		}); ok {
			upstreamIdentity = identitySession.IdentityData()
		}
	}
	if err := destination.Flush(); err != nil {
		return err
	}
	outputBatch := make([]packet.Packet, 0, localRelayBatchPacketLimit)
	flushOutputBatch := func() error {
		if len(outputBatch) == 0 {
			return nil
		}
		err := destination.WritePacketImmediate(outputBatch...)
		outputBatch = outputBatch[:0]
		return err
	}
	writePacket := func(value packet.Packet) error {
		outputBatch = append(outputBatch, value)
		if len(outputBatch) != localRelayBatchPacketLimit {
			return nil
		}
		return flushOutputBatch()
	}
	var pendingInitialStart packet.Packet
	for {
		batch, err := source.ReadBatch()
		if err != nil {
			if pendingInitialStart != nil {
				if writeErr := writePacket(pendingInitialStart); writeErr != nil {
					return errors.Join(err, writeErr)
				}
				if flushErr := flushOutputBatch(); flushErr != nil {
					return errors.Join(err, flushErr)
				}
			}
			return err
		}
		if pendingInitialStart != nil {
			if len(batch) != 0 && isLoadingScreen(batch[0], packet.LoadingScreenTypeEnd) {
				pendingInitialStart = nil
				dropInitialSpawnLoadingScreens = false
				batch = batch[1:]
			} else {
				if err := writePacket(pendingInitialStart); err != nil {
					return err
				}
				if err := flushOutputBatch(); err != nil {
					return err
				}
				pendingInitialStart = nil
				dropInitialSpawnLoadingScreens = false
			}
		}
		for _, value := range batch {
			if fromDownstream {
				value = normalizeUpstreamChatIdentity(value, upstreamIdentity)
			}
			if !fromDownstream && cacheTelemetry != nil {
				cacheTelemetry.observeRelayPacket(value)
			}
			// Each gophertunnel side performs its own initial spawn handshake. The
			// downstream listener defers ServerBoundLoadingScreen packets because it
			// does not handle them internally; forwarding those two acknowledgements
			// after the spawn barrier repeats the upstream client's acknowledgements
			// and BDS disconnects with UnexpectedPacket. The Phase-0 clients emit an
			// adjacent no-ID Start/End pair. Buffer Start until End proves that exact
			// pair; any mismatch disables the filter and preserves FIFO.
			if dropInitialSpawnLoadingScreens {
				if pendingInitialStart == nil {
					if isLoadingScreen(value, packet.LoadingScreenTypeStart) {
						pendingInitialStart = value
						continue
					}
					dropInitialSpawnLoadingScreens = false
				} else if isLoadingScreen(value, packet.LoadingScreenTypeEnd) {
					pendingInitialStart = nil
					dropInitialSpawnLoadingScreens = false
					continue
				} else {
					if err := writePacket(pendingInitialStart); err != nil {
						return err
					}
					pendingInitialStart = nil
					dropInitialSpawnLoadingScreens = false
				}
			}
			if err := writePacket(value); err != nil {
				return err
			}
		}
		// Keep one no-ID Start pending across exactly the next source batch. If
		// that batch does not begin with the matching End, the pending packet is
		// flushed as its own original batch before any current-batch packet.
		if err := flushOutputBatch(); err != nil {
			return err
		}
	}
}

func normalizeUpstreamChatIdentity(value packet.Packet, identity login.IdentityData) packet.Packet {
	text, ok := value.(*packet.Text)
	if !ok || text.TextType != packet.TextTypeChat || identity.DisplayName == "" {
		return value
	}

	rewritten := *text
	rewritten.SourceName = identity.DisplayName
	rewritten.XUID = identity.XUID
	return &rewritten
}

func isLoadingScreen(value packet.Packet, loadingType int32) bool {
	loading, ok := value.(*packet.ServerBoundLoadingScreen)
	if !ok || loading.Type != loadingType {
		return false
	}
	_, hasID := loading.LoadingScreenID.Value()
	return !hasID
}

func callWithoutPanic(call func() error) (err error) {
	defer func() {
		if recovered := recover(); recovered != nil {
			err = fmt.Errorf("panic: %v", recovered)
		}
	}()
	return call()
}

func isOrdinaryClose(err error) bool {
	if err == nil {
		return false
	}
	if terminal, ok := err.(interface{ TerminalClose() bool }); ok && terminal.TerminalClose() {
		return true
	}
	if joined, ok := err.(interface{ Unwrap() []error }); ok {
		children := joined.Unwrap()
		if len(children) == 0 {
			return false
		}
		for _, child := range children {
			if !isOrdinaryClose(child) {
				return false
			}
		}
		return true
	}
	if wrapped, ok := err.(interface{ Unwrap() error }); ok {
		if child := wrapped.Unwrap(); child != nil {
			return isOrdinaryClose(child)
		}
	}
	return errors.Is(err, io.EOF) || errors.Is(err, net.ErrClosed) || errors.Is(err, context.Canceled)
}
