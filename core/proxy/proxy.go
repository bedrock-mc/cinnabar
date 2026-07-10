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
)

const pinnedProtocol = 1001

// Config configures a local bridge listener and its upstream Bedrock server.
type Config struct {
	SocketDir string
	Upstream  string
}

// Serve listens for local bridge clients until ctx is cancelled. Session
// setup failures are returned; ordinary peer disconnects leave the listener
// available for another client.
func Serve(ctx context.Context, cfg Config) error {
	if cfg.SocketDir == "" {
		return errors.New("proxy: socket directory is required")
	}
	if cfg.Upstream == "" {
		return errors.New("proxy: upstream address is required")
	}
	if got := minecraft.DefaultProtocol.ID(); got != pinnedProtocol {
		return fmt.Errorf("proxy: protocol drift: got %d, want %d", got, pinnedProtocol)
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
	defer cancel()
	defer listener.Close()

	type acceptResult struct {
		conn net.Conn
		err  error
	}
	accepted := make(chan acceptResult)
	go func() {
		for {
			conn, err := listener.Accept()
			select {
			case accepted <- acceptResult{conn: conn, err: err}:
			case <-serveCtx.Done():
				if conn != nil {
					_ = conn.Close()
				}
				return
			}
			if err != nil {
				return
			}
		}
	}()

	sessionErr := make(chan error, 1)
	var sessions sync.WaitGroup
	stop := func() {
		cancel()
		_ = listener.Close()
		sessions.Wait()
	}
	for {
		select {
		case <-ctx.Done():
			stop()
			return nil
		case result := <-accepted:
			if result.err != nil {
				if serveCtx.Err() != nil || errors.Is(result.err, net.ErrClosed) {
					stop()
					return nil
				}
				stop()
				return fmt.Errorf("proxy: accept: %w", result.err)
			}
			downstream, ok := result.conn.(*minecraft.Conn)
			if !ok {
				_ = result.conn.Close()
				stop()
				return fmt.Errorf("proxy: accepted unexpected connection type %T", result.conn)
			}
			sessions.Add(1)
			go func() {
				defer sessions.Done()
				err := callWithoutPanic(func() error {
					return handleConnection(serveCtx, downstream, cfg.Upstream)
				})
				if err != nil && !isOrdinaryClose(err) {
					select {
					case sessionErr <- err:
					default:
					}
				}
			}()
		case err := <-sessionErr:
			stop()
			return err
		}
	}
}

func handleConnection(ctx context.Context, downstream *minecraft.Conn, upstreamAddress string) error {
	identity := downstream.IdentityData()
	offlineIdentity := login.IdentityData{
		Identity:    identity.Identity,
		DisplayName: identity.DisplayName,
	}
	upstream, err := (minecraft.Dialer{
		ClientData:   downstream.ClientData(),
		ErrorLog:     slog.Default().With("component", "upstream-dialer"),
		IdentityData: offlineIdentity,
		Protocol:     downstream.Proto(),
	}).DialContextNetwork(ctx, minecraft.RakNet{}, upstreamAddress)
	if err != nil {
		return finishDialFailure(downstream, err)
	}
	return serveConnections(ctx, downstream, upstream)
}

func finishDialFailure(downstream packetSession, dialErr error) error {
	return errors.Join(fmt.Errorf("proxy: dial upstream: %w", dialErr), closeSession(downstream))
}

type packetSession interface {
	ReadPacket() (packet.Packet, error)
	WritePacket(packet.Packet) error
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
		err = errors.Join(err, closeSession(downstream), closeSession(upstream))
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
		results <- result{"downstream to upstream", pumpPackets(downstream, upstream)}
	}()
	go func() {
		results <- result{"upstream to downstream", pumpPackets(upstream, downstream)}
	}()

	var first result
	select {
	case first = <-results:
	case <-ctx.Done():
		first = result{direction: "relay context", err: ctx.Err()}
	}
	closeErr := errors.Join(closeSession(downstream), closeSession(upstream))

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

func pumpPackets(source, destination packetSession) (err error) {
	defer func() {
		if recovered := recover(); recovered != nil {
			err = fmt.Errorf("panic while relaying packets: %v", recovered)
		}
	}()
	for {
		value, err := source.ReadPacket()
		if err != nil {
			return err
		}
		if err := destination.WritePacket(value); err != nil {
			return err
		}
	}
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
	return errors.Is(err, io.EOF) ||
		errors.Is(err, net.ErrClosed) ||
		errors.Is(err, context.Canceled)
}
