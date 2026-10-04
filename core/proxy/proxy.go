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
	"strings"
	"sync"
	"time"

	"github.com/google/uuid"
	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"github.com/sandertv/gophertunnel/minecraft/resource"
	"golang.org/x/oauth2"
)

// Config configures a local bridge listener and its upstream Bedrock server.
type Config struct {
	SocketDir string
	Upstream  string
	Account   *authcache.Account // nil runs offline
	Logger    *slog.Logger
	// UpstreamClientCache advertises blob-cache support upstream; set it only when the downstream
	// client owns a verified blob cache, since there is no runtime negotiation.
	UpstreamClientCache bool
	// ResourcePackCache is an optional process-owned cache. Serve never closes it.
	ResourcePackCache minecraft.ResourcePackCache
	// ResourcePackAdmission receives one secret-safe final snapshot per upstream
	// preparation attempt. Callbacks must return promptly.
	ResourcePackAdmission func(ResourcePackAdmissionSnapshot)
	// ConnectProgress receives the join's live stage and pack download progress.
	ConnectProgress func(ConnectProgress)
	// ResourcePackAdmissionUpdate receives an initial reset snapshot and the
	// final snapshot for each attempt. It is intended for latest-status stores.
	ResourcePackAdmissionUpdate func(ResourcePackAdmissionSnapshot)
	// Transfers, when set, receives server-directed transfers; the next local client
	// connection then dials the recorded target instead of Upstream.
	Transfers *TransferState
	// Selector, when set, supplies a client-chosen upstream that outranks LocalTarget and Upstream.
	Selector *UpstreamSelector
	// OnDisconnect receives the server's disconnect reason, before or during a session.
	OnDisconnect func(DisconnectInfo)
	// LocalTarget, when set, is asked per connection for a local server address; ok=false
	// falls back to Upstream. Upstream may then be empty.
	LocalTarget LocalTargetFunc
}

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
	if cfg.Upstream == "" && cfg.LocalTarget == nil && cfg.Selector == nil {
		return errors.New("proxy: upstream address is required")
	}
	serveCtx, cancel := context.WithCancel(ctx)
	defer cancel()
	sessionErr := make(chan error, 1)
	prepared := newPreparedConnections(cfg.Upstream, cfg.Account, logger)
	prepared.resourcePackCache = cfg.ResourcePackCache
	prepared.resourcePackAdmission = cfg.ResourcePackAdmission
	prepared.resourcePackAdmissionUpdate = cfg.ResourcePackAdmissionUpdate
	prepared.connectProgress = cfg.ConnectProgress
	prepared.upstreamClientCache = cfg.UpstreamClientCache
	transfers := cfg.Transfers
	if transfers == nil {
		transfers = new(TransferState)
	}
	dial := func(ctx context.Context, address string) (*resolvedUpstreamTarget, error) {
		return resolveUpstreamTarget(ctx, address, cfg.Account, logger)
	}
	online := func(ctx context.Context) (*resolvedUpstreamTarget, error) {
		return dial(ctx, cfg.Upstream)
	}
	prepared.dialTarget = consumeTransferOnDial(prepared.dialTarget, transfers)
	prepared.resolveTarget = withPendingTransfer(transfers, dial, withSelectedTarget(cfg.Selector, dial, withLocalTarget(cfg.LocalTarget, online)))
	listener, err := localListenConfig(func(ctx context.Context, conn *minecraft.Conn) error {
		selected, pinned := conn.Proto(), minecraft.DefaultProtocol
		clientVersion := conn.ClientData().GameVersion
		if selected.ID() != pinned.ID() || selected.Ver() != pinned.Ver() || clientVersion != pinned.Ver() {
			logger.Warn("unsupported local protocol", "protocol", selected.ID(), "version", clientVersion)
			return fmt.Errorf("unsupported local protocol %d/%s; want %d/%s", selected.ID(), clientVersion, pinned.ID(), pinned.Ver())
		}
		prepareErr := prepared.prepare(ctx, conn)
		if prepareErr != nil && serveCtx.Err() == nil {
			relayPreLoginDisconnect(conn, prepareErr)
			reportDisconnect(cfg.OnDisconnect, prepareErr)
		}
		reportPreparationError(sessionErr, prepareErr, serveCtx)
		return prepareErr
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
			// Wrapped only now: pack-stack capture needs the concrete upstream Conn.
			upstream.upstream = observeDisconnects(observeTransfers(upstream.upstream, transfers, logger), cfg.OnDisconnect)
			sessions.Add(1)
			go func() {
				defer sessions.Done()
				err := serveAcceptedConnection(serveCtx, downstream, upstream, cfg.SocketDir, logger)
				if err != nil && !streamnet.IsClosed(err) {
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

// localListenConfig configures the private same-machine listener the Rust client joins.
func localListenConfig(prepare func(context.Context, *minecraft.Conn) error) minecraft.ListenConfig {
	return minecraft.ListenConfig{
		FlushRate:              -1, // the relay's packet readers own flushing
		AuthenticationDisabled: true,
		AcceptedProtocols:      []minecraft.Protocol{minecraft.DefaultProtocol},
		AllowUnknownPackets:    true,
		EnableBatchReading:     true,
		// Same-machine traffic gains nothing from DEFLATE or AES; the upstream leg keeps both.
		Compression:             packet.NopCompression,
		DisablePacketEncryption: true,
		// The client requests one chunk at a time, so larger chunks cut loopback round trips.
		ResourcePackDelivery:     minecraft.ResourcePackDeliveryConfig{ChunkSize: localResourcePackChunkSize},
		ErrorLog:                 slog.Default().With("component", "local-listener"),
		PrepareResourcePackOffer: prepare,
	}
}

// localResourcePackChunkSize matches the Rust client's MAX_RESOURCE_PACK_CHUNK_BYTES cap.
const localResourcePackChunkSize = 1 << 20

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
	prepared.downstream = downstream
	defer func() {
		if recovered := recover(); recovered != nil {
			err = errors.Join(err, panicTypeError("starting prepared downstream session", recovered), prepared.close())
		}
	}()
	reportLocalClientAccepted(logger, socketDir, downstream.ClientCacheEnabled())
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
		upstream.packAdmission.observeLocalHandoff(upstream.packStack)
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
		abortErr = callSafely("aborting accepted connection", abortable.Abort)
	}
	return errors.Join(abortErr, callSafely("closing accepted connection", conn.Close))
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
			err = errors.Join(err, finishPreparedResources(owned, nil))
		}
	}()
	logger.Info("upstream connection starting", "target", address, "authentication", authentication)
	upstream, err := dialFollowingTransfers(ctx, address, func(ctx context.Context, address string) (upstreamSession, error) {
		upstream, dialErr := dial(ctx, address)
		if dialErr != nil && upstream != nil {
			dialErr = errors.Join(dialErr, finishPreparedResources(upstream, nil))
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

// networkForAddress keeps the resolved transport for the target itself; a server transfer
// names a plain host:port, which is always RakNet.
func networkForAddress(target *resolvedUpstreamTarget, address string) minecraft.Network {
	if strings.EqualFold(address, target.address) {
		return target.network
	}
	return minecraft.RakNet{}
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
	return transferAddress(transfer.Address, transfer.Port)
}

type dialerDownstream interface {
	IdentityData() login.IdentityData
	ClientData() login.ClientData
	Proto() minecraft.Protocol
}

func newUpstreamDialerForAdmission(
	downstream dialerDownstream,
	tokenSource oauth2.TokenSource,
	resourcePackCache minecraft.ResourcePackCache,
	packAdmission *resourcePackAdmissionTelemetry,
	enableUpstreamClientCache bool,
) minecraft.Dialer {
	dialer := minecraft.Dialer{
		ClientData:           downstream.ClientData(),
		DownloadResourcePack: ignoreResourcePack,
		ResourcePackDownload: boundedResourcePackDownload(),
		EnableBatchReading:   true,
		FlushRate:            -1, // the relay's packet readers own flushing
		// The Rust client owns the spawn sequence; the server's startup reaches it unchanged.
		RelayStartup: true,
		// A static opt-in, not the downstream status: the upstream login completes before it arrives.
		EnableClientCache: enableUpstreamClientCache,
		ErrorLog:          secretSafeResourcePackLogger(),
		Protocol:          downstream.Proto(),
		TokenSource:       tokenSource,
		ResourcePackCache: resourcePackCache,
	}
	if packAdmission != nil {
		dialer.PacketFunc = func(header packet.Header, _ []byte, _, _ net.Addr) {
			if header.PacketID == packet.IDResourcePacksInfo {
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

// ignoreResourcePack is the default until connect installs an acquisition
// budget: ignored packs stay observable in the offer and stack, and login
// continues even when the upstream required bit is set.
func ignoreResourcePack(_ uuid.UUID, _ string, _, _ int) bool { return false }

func boundedResourcePackDownload() minecraft.ResourcePackDownloadConfig {
	return minecraft.ResourcePackDownloadConfig{
		MaxInFlightChunks: minecraft.DefaultResourcePackMaxInFlightChunks,
	}
}

type packetSession interface {
	ReadBatch() ([]packet.Packet, error)
	WritePacket(packet.Packet) error
	WritePacketImmediate(...packet.Packet) error // only the final Disconnect, which bypasses deferral
	Flush() error
	Abort() error
	Close() error
}

type downstreamSession interface {
	packetSession
}

type upstreamSession interface {
	packetSession
	IdentityData() login.IdentityData // canonical account identity; wrappers must keep forwarding it
	ResourcePacks() []*resource.Pack
	TexturePacksRequired() bool
}

// relayPackets preserves batches and drains terminal delivery before asking the owner to close.
func relayPackets(
	ctx context.Context,
	downstream, upstream packetSession,
	stop func(),
) error {
	type result struct {
		direction string
		err       error
	}
	results := make(chan result, 2)
	go func() {
		results <- result{"downstream to upstream", pumpPackets(downstream, upstream, true)}
	}()
	go func() {
		results <- result{"upstream to downstream", pumpPackets(upstream, downstream, false)}
	}()

	var first result
	select {
	case first = <-results:
	case <-ctx.Done():
		first = result{direction: "relay context", err: ctx.Err()}
	}
	var second result
	var receivedBoth bool
	var upstreamClose *upstreamRelayClose
	if first.direction == "downstream to upstream" && errors.As(first.err, &upstreamClose) {
		// A closed upstream writer does not consume its queued inbound batches.
		select {
		case second = <-results:
			first, second = second, first
			receivedBoth = true
		case <-ctx.Done():
		}
	}
	var delivery <-chan error
	var deliveryErr error
	var disconnect *upstreamRelayDisconnect
	if errors.As(first.err, &disconnect) {
		completed := make(chan error, 1)
		delivery = completed
		go func() {
			completed <- callSafely("delivering disconnect", func() error {
				return downstream.WritePacketImmediate(&disconnect.value)
			})
		}()
		select {
		case deliveryErr = <-delivery:
			delivery = nil
		case <-ctx.Done():
			deliveryErr = ctx.Err()
		}
	}
	stop()
	closeErr := deliveryErr
	if delivery != nil {
		closeErr = errors.Join(closeErr, <-delivery)
	}

	if first.direction == "relay context" {
		one := <-results
		two := <-results
		second = result{direction: one.direction + " and " + two.direction, err: errors.Join(one.err, two.err)}
	} else if !receivedBoth {
		second = <-results
	}

	if ctx.Err() != nil {
		return errors.Join(ctx.Err(), closeErr)
	}
	var relayErr error
	for _, result := range []result{first, second} {
		if result.err != nil && !streamnet.IsClosed(result.err) {
			relayErr = errors.Join(relayErr, fmt.Errorf("proxy: relay %s: %w", result.direction, result.err))
		}
	}
	return errors.Join(relayErr, closeErr)
}

// shutdownSession unblocks I/O before Close can flush or wait.
func shutdownSession(session packetSession) error {
	if session == nil {
		return nil
	}
	return errors.Join(callSafely("aborting session", session.Abort), callSafely("closing session", session.Close))
}

func pumpPackets(
	source, destination packetSession,
	fromDownstream bool,
) (err error) {
	defer func() {
		if recovered := recover(); recovered != nil {
			err = panicTypeError("relaying packets", recovered)
		}
	}()
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
	reader := newPacketReader(source, destination, !fromDownstream, relayIdleFlush)
	defer reader.Close()
	// Packets buffered before the relay began leave as their own batch.
	if err := reader.Flush(); err != nil {
		return err
	}
	for {
		// One network batch in, one network batch out: see docs/relay-batch-boundaries.md.
		batch, err := reader.Read()
		if err != nil {
			return err
		}
		for _, value := range batch {
			if fromDownstream {
				value = normalizeUpstreamChatIdentity(value, upstreamIdentity)
			}
			if err := destination.WritePacket(value); err != nil {
				return attributeRelayError(err, fromDownstream)
			}
		}
		if err := reader.Flush(); err != nil {
			return err
		}
	}
}

// relayIdleFlush bounds how long a packet written outside a forwarded batch stays buffered;
// it is gophertunnel's default flush rate, which both relay legs disable.
const relayIdleFlush = time.Second / 20

type batchReadResult struct {
	packets []packet.Packet
	err     error
}

// packetReader returns source's network batches one at a time and owns every flush of
// destination, so a batch is never cut by a timer or a write inside packet handling.
type packetReader struct {
	destination packetSession
	upstream    bool // the source is upstream, which attributes its errors
	results     <-chan batchReadResult
	idle        *time.Ticker
	done        chan struct{}
}

func newPacketReader(source, destination packetSession, upstream bool, idle time.Duration) *packetReader {
	// Unbuffered: a stalled destination holds at most one batch read ahead.
	results, done := make(chan batchReadResult), make(chan struct{})
	go func() {
		defer close(results)
		for {
			packets, err := callBatchRead(source)
			select {
			case results <- batchReadResult{packets: packets, err: err}:
			case <-done:
				return
			}
			if err != nil {
				return
			}
		}
	}()
	return &packetReader{destination: destination, upstream: upstream, results: results, idle: time.NewTicker(idle), done: done}
}

func callBatchRead(source packetSession) (packets []packet.Packet, err error) {
	defer func() {
		if recovered := recover(); recovered != nil {
			err = panicTypeError("reading packets", recovered)
		}
	}()
	return source.ReadBatch()
}

// Read returns the next source batch, serving the idle flush while it waits.
func (reader *packetReader) Read() ([]packet.Packet, error) {
	for {
		select {
		case result, ok := <-reader.results:
			if !ok {
				return nil, net.ErrClosed
			}
			if result.err != nil {
				return nil, attributeRelayError(result.err, reader.upstream)
			}
			return result.packets, nil
		case <-reader.idle.C:
			if err := reader.flushDestination(); err != nil {
				return nil, err
			}
		}
	}
}

// Flush ends the forwarded batch.
func (reader *packetReader) Flush() error {
	return reader.flushDestination()
}

func (reader *packetReader) Close() {
	reader.idle.Stop()
	close(reader.done)
}

func (reader *packetReader) flushDestination() error {
	return attributeRelayError(reader.destination.Flush(), !reader.upstream)
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

// callSafely contains boundary callback panics without formatting potentially sensitive values.
func callSafely(operation string, call func() error) (err error) {
	defer func() {
		if value := recover(); value != nil {
			err = panicTypeError(operation, value)
		}
	}()
	return call()
}
