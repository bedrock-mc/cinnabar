package proxy

import (
	"context"
	"crypto/ecdsa"
	"errors"
	"net"
	"reflect"
	"sync/atomic"

	"github.com/sandertv/gophertunnel/minecraft"
)

// preparedTransport overlaps identity-independent transport setup with authentication: the
// NetherNet probe of an addressed server and any RakNet dial. It returns the original connection
// so optional packet transport capabilities survive.
type preparedTransport struct {
	minecraft.Network
	selected  minecraft.Network // the transport chosen for address, set before done closes
	address   string
	cancel    context.CancelFunc
	done      chan struct{}
	claimed   atomic.Bool
	handedOff atomic.Bool
	disposed  atomic.Bool
	conn      net.Conn
	err       error
}

func newPreparedTransport(ctx context.Context, network minecraft.Network, address string) *preparedTransport {
	ctx, cancel := context.WithCancel(ctx)
	prepared := &preparedTransport{Network: network, address: address, cancel: cancel, done: make(chan struct{})}
	go func() {
		defer close(prepared.done)
		defer func() {
			if recovered := recover(); recovered != nil {
				prepared.err = panicTypeError("preparing upstream transport", recovered)
			}
		}()
		prepared.selected = network
		if addressed, ok := network.(addressedServerNetwork); ok {
			if prepared.selected, prepared.err = addressed.Select(ctx, address); prepared.err != nil {
				return
			}
		}
		if _, ok := prepared.selected.(identityProviderDialer); ok {
			return // NetherNet proves possession during setup, so it dials after authentication
		}
		prepared.conn, prepared.err = prepared.selected.DialContext(ctx, address)
		prepared.conn = usableTransport(prepared.conn)
		if prepared.conn == nil && prepared.err == nil {
			prepared.err = net.ErrClosed
		}
	}()
	return prepared
}

func (prepared *preparedTransport) DialContext(ctx context.Context, address string) (net.Conn, error) {
	if err := prepared.claim(ctx, address); err != nil {
		return nil, err
	}
	if _, ok := prepared.selected.(identityProviderDialer); ok {
		return dialSignedOut(ctx, prepared.selected, address)
	}
	return prepared.handOff(ctx, address)
}

// DialContextIdentityProvider presents the login identity when the probe chose NetherNet.
func (prepared *preparedTransport) DialContextIdentityProvider(ctx context.Context, address, token string, key *ecdsa.PrivateKey, identityProvider string) (net.Conn, error) {
	if err := prepared.claim(ctx, address); err != nil {
		return nil, err
	}
	if dialer, ok := prepared.selected.(identityProviderDialer); ok {
		return dialer.DialContextIdentityProvider(ctx, address, token, key, identityProvider)
	}
	return prepared.handOff(ctx, address)
}

type identityProviderDialer interface {
	DialContextIdentityProvider(ctx context.Context, address, token string, key *ecdsa.PrivateKey, identityProvider string) (net.Conn, error)
}

// claim takes the single handoff and waits for setup to finish.
func (prepared *preparedTransport) claim(ctx context.Context, address string) error {
	if address != prepared.address {
		return errors.New("proxy: prepared transport target changed")
	}
	if !prepared.claimed.CompareAndSwap(false, true) {
		return errors.New("proxy: prepared transport already claimed")
	}
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-prepared.done:
		if err := ctx.Err(); err != nil {
			return err
		}
		return prepared.err
	}
}

func (prepared *preparedTransport) handOff(ctx context.Context, address string) (net.Conn, error) {
	if connection, ok := prepared.conn.(interface{ Context() context.Context }); ok && connection.Context().Err() != nil {
		// The upstream closes an idle connection once its login deadline passes, which slow
		// authentication can outlast; finish closes the expired one and login takes a fresh dial.
		return prepared.selected.DialContext(ctx, address)
	}
	prepared.handedOff.Store(true)
	return prepared.conn, nil
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

// finish cancels unfinished setup and closes any transport the login did not retain.
func (prepared *preparedTransport) finish(retained bool) {
	prepared.cancel()
	<-prepared.done
	if (!retained || !prepared.handedOff.Load()) && prepared.disposed.CompareAndSwap(false, true) && prepared.conn != nil {
		_ = prepared.conn.Close()
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
	case minecraft.RakNet, *minecraft.RakNet, addressedServerNetwork:
		prepared := newPreparedTransport(ctx, network, address)
		defer func() { prepared.finish(connection != nil && err == nil) }()
		connection, err = dial(ctx, prepared, address)
		if err != nil && prepared.handedOff.Load() && ctx.Err() == nil && stalePreLogin(err) {
			// The upstream can expire the early connection while its shutdown is still deferred, so
			// login fails on a live-looking transport; retry once on a fresh one.
			prepared.finish(false)
			return dial(ctx, network, address)
		}
		return connection, err
	default:
		return dial(ctx, network, address)
	}
}

// stalePreLogin reports a login that failed on a closed transport rather than a server answer.
func stalePreLogin(err error) bool {
	var transfer *minecraft.TransferError
	var disconnect minecraft.DisconnectError
	if errors.As(err, &transfer) || errors.As(err, &disconnect) {
		return false
	}
	return errors.Is(err, context.Canceled) || errors.Is(err, net.ErrClosed)
}
