// Package proxy joins a local client session to an upstream
// Bedrock server and relays packets between them, decoding only those it inspects.
package proxy

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/device"
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
	PacketDelay *PacketDelay
	// ServerTrust, when set, decides whether to join NetherNet servers reached by address.
	ServerTrust minecraft.ServerTrust
	// SessionTarget, when set, maps a session Connect's connect.v1 target to a proxy target for that
	// session alone and drops any pending transfer, as connect.v1 does; nil rejects targeted Connects.
	SessionTarget func(ctx context.Context, kind, value string) (string, error)
	// Device, when set, is the install's device, claimed on every session login in place of the client's.
	Device *device.Profile
}

const maxInitialTransferHops = 8

// Serve listens for local sessions until ctx is cancelled. Listener setup and cleanup
// failures are returned; session errors are reported without ending the server.
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
	prepared := newPreparedConnections(cfg.Upstream, cfg.Account, logger)
	prepared.resourcePackCache = cfg.ResourcePackCache
	prepared.resourcePackAdmission = cfg.ResourcePackAdmission
	prepared.resourcePackAdmissionUpdate = cfg.ResourcePackAdmissionUpdate
	prepared.connectProgress = cfg.ConnectProgress
	prepared.serverTrust = cfg.ServerTrust
	prepared.upstreamClientCache = cfg.UpstreamClientCache
	transfers := cfg.Transfers
	if transfers == nil {
		transfers = new(TransferState)
	}
	dial := func(ctx context.Context, address string) (*resolvedUpstreamTarget, error) {
		return resolveUpstreamTarget(ctx, address, cfg.Account, logger, cfg.ServerTrust)
	}
	online := func(ctx context.Context) (*resolvedUpstreamTarget, error) {
		return dial(ctx, cfg.Upstream)
	}
	prepared.dialTarget = consumeTransferOnDial(prepared.dialTarget, transfers)
	prepared.resolveTarget = withPendingTransfer(transfers, dial, withSelectedTarget(cfg.Selector, dial, withLocalTarget(cfg.LocalTarget, online)))
	sessionListener, err := streamnet.ListenSession(cfg.SocketDir)
	if err != nil {
		return errors.Join(fmt.Errorf("proxy: listen for sessions: %w", err), prepared.shutdown())
	}
	sessionEndpoint := &sessionServer{
		listener:     sessionListener,
		prepared:     prepared,
		transfers:    transfers,
		onDisconnect: cfg.OnDisconnect,
		selectTarget: cfg.SessionTarget,
		device:       cfg.Device,
		dialTarget:   dial,
		delay:        cfg.PacketDelay,
		logger:       logger,
	}
	sessionEndpoint.start(serveCtx)
	reportListenerReady(logger, cfg.SocketDir)

	defer func() {
		err = errors.Join(err, shutdownPreparedServer(prepared, func() error {
			cancel()
			return sessionEndpoint.close()
		}))
	}()
	<-ctx.Done()
	return nil
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

func reportListenerReady(logger *slog.Logger, socketDir string) {
	attributes := []any{"socket_dir", socketDir}
	if network, endpoint, err := streamnet.ResolveSession(socketDir); err == nil {
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
	upstream, err := dialFollowingTransfers(ctx, address, dial)
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
// names a plain host:port, which vanilla joins like any addressed server, whatever the target was.
func networkForAddress(target *resolvedUpstreamTarget, address string, trust minecraft.ServerTrust) minecraft.Network {
	if strings.EqualFold(address, target.address) {
		return target.network
	}
	return remoteServerNetwork(slog.Default(), trust)
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
			if upstream != nil {
				err = errors.Join(err, finishPreparedResources(upstream, nil))
			}
			return nil, err
		}
		if upstream != nil {
			// The old server still gets its disconnect, but the next hop never waits for it.
			go func() { _ = finishPreparedResources(upstream, nil) }()
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
		Handoff: minecraft.HandoffAtStartGame,
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
	// ReadBatchRaw returns one network batch encoded, decoding only the packets decode selects.
	ReadBatchRaw(decode func(id uint32) bool) ([]minecraft.RawPacket, error)
	WritePacketRaw([]byte) error // forwards encoded bytes; both legs share one protocol
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
	delays ...*PacketDelay,
) error {
	var delay *PacketDelay
	if len(delays) != 0 {
		delay = delays[0]
	}
	session := delay.beginSession()
	defer delay.endSession(session)
	pumpCtx, cancel := context.WithCancel(ctx)
	defer cancel()
	type result struct {
		direction string
		err       error
	}
	results := make(chan result, 2)
	go func() {
		results <- result{"downstream to upstream", pumpPacketsWithDelay(pumpCtx, delay, downstream, upstream, true, session)}
	}()
	go func() {
		results <- result{"upstream to downstream", pumpPacketsWithDelay(pumpCtx, delay, upstream, downstream, false)}
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
	delay.endSession(session)
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
	return pumpPacketsWithDelay(context.Background(), nil, source, destination, fromDownstream)
}

func pumpPacketsWithDelay(ctx context.Context, delay *PacketDelay, source, destination packetSession, fromDownstream bool, sessions ...uint64) (err error) {
	defer func() {
		if recovered := recover(); recovered != nil {
			err = panicTypeError("relaying packets", recovered)
		}
	}()
	var upstreamIdentity login.IdentityData
	if fromDownstream {
		// Local logins carry a transport identity; the upstream connection owns
		// the server's canonical player identity.
		if identitySession, ok := destination.(interface {
			IdentityData() login.IdentityData
		}); ok {
			upstreamIdentity = identitySession.IdentityData()
		}
	}
	var inspect func(uint32) bool
	if upstreamIdentity.DisplayName != "" || upstreamIdentity.Identity != "" {
		inspect = func(id uint32) bool { return inspectsUpstreamIdentity(id, upstreamIdentity) }
	}
	var ownID, session uint64
	if fromDownstream && delay != nil {
		ownID = ownRuntimeID(source, destination)
		session, _ = delay.PositionSnapshot()
		if len(sessions) != 0 {
			session = sessions[0]
		}
		inspect = func(id uint32) bool {
			if inspectsUpstreamIdentity(id, upstreamIdentity) {
				return true
			}
			if !isOwnMovement(id) {
				return false
			}
			_, enabled := delay.positionToken()
			return enabled
		}
	}
	reader := newDelayedPacketReader(ctx, delay, source, destination, !fromDownstream, relayIdleFlush, inspect)
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
		epoch, tracking := delay.positionToken()
		var position *ForwardedPosition
		for _, raw := range batch {
			if err := forwardPacket(destination, raw, upstreamIdentity); err != nil {
				return attributeRelayError(err, fromDownstream)
			}
			if tracking && fromDownstream {
				if candidate := ownMovementPosition(raw, ownID); candidate != nil {
					position = candidate
				}
			}
		}
		if err := reader.Flush(); err != nil {
			return err
		}
		delay.commitPosition(session, epoch, position)
	}
}

func inspectsUpstreamIdentity(id uint32, identity login.IdentityData) bool {
	switch id {
	case packet.IDText:
		return identity.DisplayName != ""
	case packet.IDPlayerSkin:
		return identity.Identity != ""
	default:
		return false
	}
}

// forwardPacket writes raw's received bytes unless the proxy rewrote one of its decoded packets.
func forwardPacket(destination packetSession, raw minecraft.RawPacket, identity login.IdentityData) error {
	rewritten := len(raw.Decoded) > 1
	for _, value := range raw.Decoded {
		rewritten = rewritten || normalizeUpstreamIdentity(value, identity) != value
	}
	if !rewritten {
		return destination.WritePacketRaw(raw.Data)
	}
	for _, value := range raw.Decoded {
		if err := destination.WritePacket(normalizeUpstreamIdentity(value, identity)); err != nil {
			return err
		}
	}
	return nil
}

// relayIdleFlush bounds how long a packet written outside a forwarded batch stays buffered;
// it is gophertunnel's default flush rate, which both relay legs disable.
const relayIdleFlush = time.Second / 20

type batchReadResult struct {
	packets []minecraft.RawPacket
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
	ctx         context.Context
	delay       *PacketDelay
	pending     delayedBatches
	terminal    bool
	wake        *time.Timer
	released    uint64
}

func newPacketReader(source, destination packetSession, upstream bool, idle time.Duration, decode func(uint32) bool) *packetReader {
	return newDelayedPacketReader(context.Background(), nil, source, destination, upstream, idle, decode)
}

func newDelayedPacketReader(ctx context.Context, delay *PacketDelay, source, destination packetSession, upstream bool, idle time.Duration, decode func(uint32) bool) *packetReader {
	// Unbuffered: a stalled destination holds at most one batch read ahead.
	results, done := make(chan batchReadResult), make(chan struct{})
	go func() {
		defer close(results)
		for {
			packets, err := callBatchRead(source, decode)
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
	return &packetReader{destination: destination, upstream: upstream, results: results, idle: time.NewTicker(idle), done: done, ctx: ctx, delay: delay}
}

func callBatchRead(source packetSession, decode func(uint32) bool) (packets []minecraft.RawPacket, err error) {
	defer func() {
		if recovered := recover(); recovered != nil {
			err = panicTypeError("reading packets", recovered)
		}
	}()
	return source.ReadBatchRaw(decode)
}

// Read returns the next source batch, serving the idle flush while it waits.
func (reader *packetReader) Read() ([]minecraft.RawPacket, error) {
	defer func() {
		if reader.wake != nil {
			reader.wake.Stop()
		}
	}()
	for {
		now := time.Now()
		delay, expires, changed, released := reader.delay.snapshot(now)
		if released != reader.released {
			for index := range reader.pending.count {
				reader.pending.items[(reader.pending.head+index)%maxDelayedBatches].due = time.Time{}
			}
			reader.released = released
		}
		var wake <-chan time.Time
		if reader.pending.count != 0 {
			if delay == 0 || !now.Before(reader.pending.front().due) {
				result := reader.pending.pop()
				return result.packets, attributeRelayError(result.err, reader.upstream)
			}
			deadline := reader.pending.front().due
			if expires.Before(deadline) {
				deadline = expires
			}
			if reader.wake == nil {
				reader.wake = time.NewTimer(deadline.Sub(now))
			} else {
				reader.wake.Reset(deadline.Sub(now))
			}
			wake = reader.wake.C
		}
		results := reader.results
		if reader.pending.full() || reader.terminal {
			results = nil
		}
		select {
		case <-reader.ctx.Done():
			return nil, reader.ctx.Err()
		case <-changed:
		case <-wake:
		case result, ok := <-results:
			if !ok {
				result.err = net.ErrClosed
			}
			if delay == 0 && reader.pending.count == 0 {
				return result.packets, attributeRelayError(result.err, reader.upstream)
			}
			reader.pending.push(result, time.Now().Add(delay))
			reader.terminal = result.err != nil
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
	if reader.wake != nil {
		reader.wake.Stop()
	}
	reader.idle.Stop()
	close(reader.done)
}

func (reader *packetReader) flushDestination() error {
	return attributeRelayError(reader.destination.Flush(), !reader.upstream)
}

func normalizeUpstreamIdentity(value packet.Packet, identity login.IdentityData) packet.Packet {
	switch value := value.(type) {
	case *packet.Text:
		if value.TextType != packet.TextTypeChat || identity.DisplayName == "" {
			return value
		}
		rewritten := *value
		rewritten.SourceName = identity.DisplayName
		rewritten.XUID = identity.XUID
		return &rewritten
	case *packet.PlayerSkin:
		canonical, err := uuid.Parse(identity.Identity)
		if err != nil || value.UUID == canonical {
			return value
		}
		rewritten := *value
		rewritten.UUID = canonical
		return &rewritten
	default:
		return value
	}
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
