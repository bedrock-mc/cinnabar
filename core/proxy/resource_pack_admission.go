package proxy

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"sync"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"github.com/sandertv/gophertunnel/minecraft/resource"
	"golang.org/x/oauth2"
)

// PackAdmissionFailureReason classifies a resource-pack failure without
// exposing pack identifiers, URLs, or content keys.
type PackAdmissionFailureReason uint8

const (
	// PackAdmissionRequiredUnsupported means the upstream requires one or more
	// packs that Cinnabar cannot truthfully apply yet.
	PackAdmissionRequiredUnsupported PackAdmissionFailureReason = iota + 1
)

// PackAdmissionError reports a typed, bounded pre-login pack failure.
type PackAdmissionError struct {
	Reason    PackAdmissionFailureReason
	PackCount int
}

type preparationCancellationError struct {
	cause error
}

func (*preparationCancellationError) Error() string {
	return "proxy: preparation cancelled by local shutdown or downstream peer"
}

func (err *preparationCancellationError) Unwrap() error { return err.cause }

func (err *PackAdmissionError) Error() string {
	return fmt.Sprintf("proxy: upstream requires %d resource pack(s), but pack application is unavailable", err.PackCount)
}

type resourcePackOfferConnection interface {
	dialerDownstream
	ConfigureResourcePackOffer([]*resource.Pack, bool) error
	WritePacketImmediate(...packet.Packet) error
}

const packAdmissionDisconnectMessage = "This server requires resource packs that Cinnabar cannot apply yet."

// configureResourcePackOffer applies Cinnabar's current truthful policy. An
// optional offer is downloaded under explicit bounds by the upstream Dialer,
// retained with that upstream session, and stripped from the downstream offer.
// A non-empty required offer is rejected because Cinnabar cannot apply it yet.
func configureResourcePackOffer(downstream resourcePackOfferConnection, upstream upstreamSession) error {
	packs := upstream.ResourcePacks()
	if upstream.TexturePacksRequired() && len(packs) != 0 {
		admissionErr := &PackAdmissionError{
			Reason:    PackAdmissionRequiredUnsupported,
			PackCount: len(packs),
		}
		writeErr := downstream.WritePacketImmediate(&packet.Disconnect{
			Reason:                  packet.DisconnectReasonResourcePackProblem,
			HideDisconnectionScreen: false,
			Message:                 packAdmissionDisconnectMessage,
		})
		return errors.Join(admissionErr, writeErr)
	}
	return downstream.ConfigureResourcePackOffer(nil, false)
}

// preparedConnection owns every resource created while preparing one exact
// downstream connection. close is idempotent so cancellation and listener
// shutdown cannot double-close an upstream session or target.
type preparedConnection struct {
	upstream      upstreamSession
	releaseTarget func() error
	telemetry     *cacheBoundaryTelemetry
	logger        *slog.Logger

	closeOnce sync.Once
	closeErr  error
}

func (prepared *preparedConnection) close() error {
	return prepared.finish(true)
}

func (prepared *preparedConnection) releaseAfterRelay() error {
	return prepared.finish(false)
}

func (prepared *preparedConnection) finish(shutdownUpstream bool) error {
	if prepared == nil {
		return nil
	}
	prepared.closeOnce.Do(func() {
		prepared.closeErr = finishPreparedResources(
			shutdownUpstream,
			prepared.upstream,
			prepared.releaseTarget,
			prepared.telemetry,
			prepared.logger,
		)
	})
	return prepared.closeErr
}

func finishPreparedResources(
	shutdownUpstream bool,
	upstream upstreamSession,
	releaseTarget func() error,
	telemetry *cacheBoundaryTelemetry,
	logger *slog.Logger,
) error {
	var err error
	if shutdownUpstream && upstream != nil {
		err = errors.Join(err,
			callPreparedCleanup("aborting upstream", upstream.Abort),
			callPreparedCleanup("closing upstream", upstream.Close),
		)
	}
	if releaseTarget != nil {
		err = errors.Join(err, callPreparedCleanup("closing upstream target", releaseTarget))
	}
	if telemetry != nil {
		err = errors.Join(err, callPreparedCleanup("reporting upstream telemetry", func() error {
			telemetry.report(logger)
			return nil
		}))
	}
	return err
}

func callPreparedCleanup(operation string, call func() error) (err error) {
	defer func() {
		if recovered := recover(); recovered != nil {
			err = panicTypeError(operation, recovered)
		}
	}()
	return call()
}

func panicTypeError(operation string, recovered any) error {
	return fmt.Errorf("proxy: panic while %s (type %T)", operation, recovered)
}

type preparedSlot struct {
	connection *preparedConnection
	detached   chan struct{}
}

// preparedConnections retains a prepared upstream by the exact downstream
// *minecraft.Conn identity until Accept transfers ownership to the session.
type preparedConnections struct {
	tokenSource     oauth2.TokenSource
	logger          *slog.Logger
	connectPrepared func(context.Context, dialerDownstream) (*preparedConnection, error)
	resolveTarget   func(context.Context) (*resolvedUpstreamTarget, error)
	dialTarget      func(context.Context, *resolvedUpstreamTarget, minecraft.Dialer) (upstreamSession, error)

	shutdownCtx    context.Context
	shutdownCancel context.CancelFunc
	beginStopOnce  sync.Once
	finishStopOnce sync.Once
	shutdownErr    error
	prepareWG      sync.WaitGroup
	cleanupWG      sync.WaitGroup

	mu       sync.Mutex
	stopping bool
	entries  map[*minecraft.Conn]*preparedSlot
}

func newPreparedConnections(upstreamAddress string, tokenSource oauth2.TokenSource, logger *slog.Logger) *preparedConnections {
	shutdownCtx, shutdownCancel := context.WithCancel(context.Background())
	connections := &preparedConnections{
		tokenSource:    tokenSource,
		logger:         logger,
		shutdownCtx:    shutdownCtx,
		shutdownCancel: shutdownCancel,
		entries:        make(map[*minecraft.Conn]*preparedSlot),
	}
	connections.connectPrepared = connections.connect
	connections.resolveTarget = func(ctx context.Context) (*resolvedUpstreamTarget, error) {
		return resolveUpstreamTarget(ctx, upstreamAddress, tokenSource, logger)
	}
	connections.dialTarget = func(ctx context.Context, target *resolvedUpstreamTarget, dialer minecraft.Dialer) (upstreamSession, error) {
		return connectUpstream(ctx, target.address, authenticationMode(tokenSource), logger, func(ctx context.Context, address string) (upstreamSession, error) {
			return dialer.DialContextNetwork(ctx, target.network, address)
		})
	}
	return connections
}

func (connections *preparedConnections) prepare(ctx context.Context, downstream *minecraft.Conn) error {
	return connections.prepareConnection(ctx, downstream, downstream)
}

func (connections *preparedConnections) prepareConnection(
	ctx context.Context,
	key *minecraft.Conn,
	downstream resourcePackOfferConnection,
) (err error) {
	connections.mu.Lock()
	if connections.stopping {
		connections.mu.Unlock()
		return context.Canceled
	}
	connections.prepareWG.Add(1)
	connections.mu.Unlock()
	defer connections.prepareWG.Done()
	defer func() {
		if err == nil || (ctx.Err() == nil && connections.shutdownCtx.Err() == nil) {
			return
		}
		err = &preparationCancellationError{cause: err}
	}()

	prepareCtx, cancel := context.WithCancel(ctx)
	stopShutdownCancellation := context.AfterFunc(connections.shutdownCtx, cancel)
	defer func() {
		stopShutdownCancellation()
		cancel()
	}()

	prepared, err := connections.connectPrepared(prepareCtx, downstream)
	if err != nil {
		return errors.Join(err, prepared.close())
	}
	owned := true
	defer func() {
		if recovered := recover(); recovered != nil {
			err = errors.Join(err, panicTypeError("configuring downstream resource-pack offer", recovered))
		}
		if owned {
			err = errors.Join(err, prepared.close())
		}
	}()
	if err = configureResourcePackOffer(downstream, prepared.upstream); err != nil {
		return err
	}
	if err = connections.store(ctx, key, prepared); err != nil {
		return err
	}
	owned = false
	return nil
}

func (connections *preparedConnections) connect(ctx context.Context, downstream dialerDownstream) (result *preparedConnection, err error) {
	telemetry := new(cacheBoundaryTelemetry)
	var target *resolvedUpstreamTarget
	var upstream upstreamSession
	defer func() {
		if recovered := recover(); recovered != nil {
			err = panicTypeError("preparing upstream connection", recovered)
			result = nil
		}
		if result != nil {
			return
		}
		var releaseTarget func() error
		if target != nil {
			releaseTarget = target.close
		}
		err = errors.Join(err, finishPreparedResources(true, upstream, releaseTarget, telemetry, connections.logger))
	}()

	target, err = connections.resolveTarget(ctx)
	if err != nil {
		return nil, err
	}
	dialer := newUpstreamDialerWithCacheTelemetry(downstream, connections.tokenSource, telemetry)
	if target.xbl != nil {
		dialer.XBLClient = target.xbl
	}
	if target.playFab != nil {
		dialer.PlayFabClient = target.playFab
	}
	if target.clientData.nonce != "" {
		dialer.ClientData.Nonce = target.clientData.nonce
	}
	upstream, err = connections.dialTarget(ctx, target, dialer)
	if err != nil {
		return nil, err
	}
	result = &preparedConnection{
		upstream:      upstream,
		releaseTarget: target.close,
		telemetry:     telemetry,
		logger:        connections.logger,
	}
	return result, nil
}

func (connections *preparedConnections) store(ctx context.Context, downstream *minecraft.Conn, prepared *preparedConnection) error {
	if downstream == nil || prepared == nil {
		return errors.New("proxy: cannot retain nil prepared connection")
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	slot := &preparedSlot{connection: prepared, detached: make(chan struct{})}
	connections.mu.Lock()
	if connections.stopping {
		connections.mu.Unlock()
		return context.Canceled
	}
	if err := ctx.Err(); err != nil {
		connections.mu.Unlock()
		return err
	}
	if _, exists := connections.entries[downstream]; exists {
		connections.mu.Unlock()
		return errors.New("proxy: duplicate prepared upstream for downstream connection")
	}
	connections.entries[downstream] = slot
	connections.mu.Unlock()

	go func() {
		select {
		case <-ctx.Done():
			_ = connections.discard(downstream, slot)
		case <-slot.detached:
		}
	}()
	return nil
}

func (connections *preparedConnections) take(downstream *minecraft.Conn) (*preparedConnection, bool) {
	connections.mu.Lock()
	slot, ok := connections.entries[downstream]
	if ok {
		delete(connections.entries, downstream)
		close(slot.detached)
	}
	connections.mu.Unlock()
	if !ok {
		return nil, false
	}
	return slot.connection, true
}

func (connections *preparedConnections) discard(downstream *minecraft.Conn, expected *preparedSlot) error {
	connections.mu.Lock()
	slot, ok := connections.entries[downstream]
	if !ok || slot != expected {
		connections.mu.Unlock()
		return nil
	}
	delete(connections.entries, downstream)
	close(slot.detached)
	connections.cleanupWG.Add(1)
	connections.mu.Unlock()
	defer connections.cleanupWG.Done()
	return slot.connection.close()
}

func (connections *preparedConnections) beginShutdown() {
	connections.beginStopOnce.Do(func() {
		connections.mu.Lock()
		connections.stopping = true
		connections.shutdownCancel()
		connections.mu.Unlock()
	})
}

func (connections *preparedConnections) finishShutdown() error {
	connections.beginShutdown()
	connections.finishStopOnce.Do(func() {
		connections.prepareWG.Wait()

		connections.mu.Lock()
		entries := make([]*preparedConnection, 0, len(connections.entries))
		for downstream, slot := range connections.entries {
			delete(connections.entries, downstream)
			close(slot.detached)
			entries = append(entries, slot.connection)
		}
		connections.mu.Unlock()
		for _, prepared := range entries {
			connections.shutdownErr = errors.Join(connections.shutdownErr, prepared.close())
		}
		connections.cleanupWG.Wait()
	})
	return connections.shutdownErr
}

func (connections *preparedConnections) shutdown() error {
	connections.beginShutdown()
	return connections.finishShutdown()
}

func servePreparedConnection(ctx context.Context, downstream downstreamSession, prepared *preparedConnection) (err error) {
	relayCompleted := false
	defer func() {
		if relayCompleted {
			err = errors.Join(err, prepared.releaseAfterRelay())
			return
		}
		err = errors.Join(err, shutdownSession(downstream), prepared.close())
	}()
	if err := spawnBarrier(ctx, downstream, prepared.upstream); err != nil {
		return err
	}
	err = relayPacketsWithCacheTelemetry(ctx, downstream, prepared.upstream, prepared.telemetry)
	relayCompleted = true
	return err
}
