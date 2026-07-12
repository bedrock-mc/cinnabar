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
	"sync"

	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"golang.org/x/oauth2"
)

// Config configures a local bridge listener and its upstream Bedrock server.
type Config struct {
	SocketDir   string
	Upstream    string
	TokenSource oauth2.TokenSource
}

const localRelayBatchPacketLimit = 1600

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
	if cfg.SocketDir == "" {
		return errors.New("proxy: socket directory is required")
	}
	if cfg.Upstream == "" {
		return errors.New("proxy: upstream address is required")
	}
	listener, err := (minecraft.ListenConfig{
		AuthenticationDisabled: true,
		AllowUnknownPackets:    true,
		ErrorLog:               slog.Default().With("component", "local-listener"),
	}).ListenNetwork(streamnet.New(cfg.SocketDir), "")
	if err != nil {
		return fmt.Errorf("proxy: listen: %w", err)
	}

	serveCtx, cancel := context.WithCancel(ctx)

	accepted := make(chan acceptResult)
	acceptDone := make(chan error, 1)
	go func() {
		acceptDone <- runAcceptLoop(serveCtx, listener, accepted)
	}()

	sessionErr := make(chan error, 1)
	var sessions sync.WaitGroup
	var stopOnce sync.Once
	var stopErr error
	stop := func() error {
		stopOnce.Do(func() {
			stopErr = stopServer(cancel, listener, &sessions, acceptDone)
		})
		return stopErr
	}
	defer func() { err = errors.Join(err, stop()) }()
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
			sessions.Add(1)
			go func() {
				defer sessions.Done()
				err := callWithoutPanic(func() error {
					return handleConnection(serveCtx, downstream, cfg.Upstream, cfg.TokenSource)
				})
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

func handleConnection(ctx context.Context, downstream *minecraft.Conn, upstreamAddress string, tokenSource oauth2.TokenSource) error {
	dialer := newUpstreamDialer(downstream, tokenSource)
	return dialAndServe(ctx, downstream, func(ctx context.Context) (upstreamSession, error) {
		return dialer.DialContextNetwork(ctx, minecraft.RakNet{}, upstreamAddress)
	})
}

type dialerDownstream interface {
	IdentityData() login.IdentityData
	ClientData() login.ClientData
	Proto() minecraft.Protocol
}

func newUpstreamDialer(downstream dialerDownstream, tokenSource oauth2.TokenSource) minecraft.Dialer {
	dialer := minecraft.Dialer{
		ClientData:  downstream.ClientData(),
		ErrorLog:    slog.Default().With("component", "upstream-dialer"),
		Protocol:    downstream.Proto(),
		TokenSource: tokenSource,
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

func dialAndServe(ctx context.Context, downstream downstreamSession, dial func(context.Context) (upstreamSession, error)) error {
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
		return serveConnections(ctx, downstream, result.upstream)
	}
}

func finishDialFailure(downstream packetSession, dialErr error) error {
	return errors.Join(fmt.Errorf("proxy: dial upstream: %w", dialErr), shutdownSession(downstream))
}

type packetSession interface {
	ReadPacket() (packet.Packet, error)
	WritePacket(packet.Packet) error
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
}

func serveConnections(ctx context.Context, downstream downstreamSession, upstream upstreamSession) (err error) {
	defer func() {
		err = errors.Join(err, shutdownSession(downstream), shutdownSession(upstream))
	}()

	if err := spawnBarrier(ctx, downstream, upstream); err != nil {
		return err
	}
	return relayPackets(ctx, downstream, upstream)
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
	defer func() {
		if recovered := recover(); recovered != nil {
			err = fmt.Errorf("panic while relaying packets: %v", recovered)
		}
	}()
	dropInitialSpawnLoadingScreens := fromDownstream
	capLocalBatches := !fromDownstream
	localBatchStarted := false
	localBatchPackets := 0
	writePacket := func(value packet.Packet) error {
		if capLocalBatches && !localBatchStarted {
			if err := destination.Flush(); err != nil {
				return err
			}
			localBatchStarted = true
		}
		if err := destination.WritePacket(value); err != nil {
			return err
		}
		if !capLocalBatches {
			return nil
		}
		localBatchPackets++
		if localBatchPackets != localRelayBatchPacketLimit {
			return nil
		}
		if err := destination.Flush(); err != nil {
			return err
		}
		localBatchPackets = 0
		return nil
	}
	var pendingInitialStart packet.Packet
	for {
		value, err := source.ReadPacket()
		if err != nil {
			if pendingInitialStart != nil {
				return errors.Join(err, writePacket(pendingInitialStart))
			}
			return err
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
