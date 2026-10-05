package proxy

import (
	"context"
	"errors"
	"net"
	"reflect"
	"sync/atomic"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
)

// preparedTransport overlaps identity-independent transport setup with authentication.
// It returns the original connection so optional packet transport capabilities survive.
type preparedTransport struct {
	minecraft.Network
	address   string
	cancel    context.CancelFunc
	done      chan struct{}
	claimed   atomic.Bool
	handedOff atomic.Bool
	disposed  atomic.Bool
	conn      net.Conn
	err       error
	dispose   func()
}

func newPreparedTransport(ctx context.Context, network minecraft.Network, address string) *preparedTransport {
	return newOwnedPreparedTransport(ctx, network, address, nil)
}

func newOwnedPreparedTransport(ctx context.Context, network minecraft.Network, address string, dispose func()) *preparedTransport {
	ctx, cancel := context.WithCancel(ctx)
	prepared := &preparedTransport{Network: network, address: address, cancel: cancel, done: make(chan struct{}), dispose: dispose}
	go func() {
		defer close(prepared.done)
		defer func() {
			if recovered := recover(); recovered != nil {
				prepared.err = panicTypeError("preparing upstream transport", recovered)
			}
		}()
		started := time.Now()
		prepared.conn, prepared.err = network.DialContext(ctx, address)
		prepared.conn = usableTransport(prepared.conn)
		if prepared.conn == nil && prepared.err == nil {
			prepared.err = net.ErrClosed
		}
		reportJoinDuration(ctx, "transport", started, prepared.err == nil)
	}()
	return prepared
}

func (prepared *preparedTransport) DialContext(ctx context.Context, address string) (net.Conn, error) {
	if address != prepared.address {
		return nil, errors.New("proxy: prepared transport target changed")
	}
	if !prepared.claimed.CompareAndSwap(false, true) {
		return nil, errors.New("proxy: prepared transport already claimed")
	}
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-prepared.done:
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		if prepared.err != nil {
			return nil, prepared.err
		}
		if connection, ok := prepared.conn.(interface{ Context() context.Context }); ok && connection.Context().Err() != nil {
			return nil, net.ErrClosed
		}
		prepared.handedOff.Store(prepared.conn != nil && prepared.err == nil)
		return prepared.conn, prepared.err
	}
}

// Network implementations may return an interface containing a nil connection.
func usableTransport(conn net.Conn) net.Conn {
	if conn == nil {
		return nil
	}
	value := reflect.ValueOf(conn)
	switch value.Kind() {
	case reflect.Chan, reflect.Func, reflect.Interface, reflect.Map, reflect.Pointer, reflect.Slice:
		if value.IsNil() {
			return nil
		}
	}
	return conn
}

func (prepared *preparedTransport) ready() bool {
	select {
	case <-prepared.done:
		if prepared.conn == nil || prepared.err != nil {
			return false
		}
		connection, scoped := prepared.conn.(interface{ Context() context.Context })
		return !scoped || connection.Context().Err() == nil
	default:
		return false
	}
}

// finish cancels unfinished setup and closes any transport the login did not retain.
func (prepared *preparedTransport) finish(retained bool) {
	prepared.cancel()
	<-prepared.done
	if (!retained || !prepared.handedOff.Load()) && prepared.disposed.CompareAndSwap(false, true) {
		if prepared.dispose != nil {
			prepared.dispose()
		}
		if prepared.conn != nil {
			_ = prepared.conn.Close()
		}
	}
}

func dialWithPreparedTransport(
	ctx context.Context,
	network minecraft.Network,
	address string,
	dial func(context.Context, minecraft.Network, string) (*minecraft.Conn, error),
) (connection *minecraft.Conn, err error) {
	// NetherNet proves possession during transport setup and must authenticate first.
	switch network.(type) {
	case minecraft.RakNet, *minecraft.RakNet:
		prepared := newPreparedTransport(ctx, network, address)
		defer func() { prepared.finish(connection != nil && err == nil) }()
		return dial(ctx, prepared, address)
	default:
		return dial(ctx, network, address)
	}
}
