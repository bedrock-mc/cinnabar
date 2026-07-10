package streamnet

import (
	"context"
	cryptorand "crypto/rand"
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"runtime"
	"sync"

	"github.com/hashimthearab/rust-mcbe/core/internal/lockfile"
	"github.com/sandertv/gophertunnel/minecraft"
)

var (
	// ErrPingUnsupported reports that the local stream transport has no server-list ping protocol.
	ErrPingUnsupported = errors.New("streamnet: ping is unsupported")
)

type network struct {
	socketDir string
}

// New returns a gophertunnel network backed by the fixed local endpoint in socketDir.
func New(socketDir string) minecraft.Network {
	return &network{socketDir: socketDir}
}

func (n *network) DialContext(ctx context.Context, address string) (net.Conn, error) {
	networkName, resolved, err := Resolve(n.socketDir)
	if err != nil {
		return nil, err
	}
	if address != "" && address != resolved {
		return nil, fmt.Errorf("streamnet: address %q does not match published endpoint %q", address, resolved)
	}
	dialer := net.Dialer{}
	conn, err := dialer.DialContext(ctx, networkName, resolved)
	if err != nil {
		return nil, err
	}
	return NewFramedConn(conn), nil
}

func (n *network) PingContext(context.Context, string) ([]byte, error) {
	return nil, ErrPingUnsupported
}

func (n *network) Listen(string) (minecraft.NetworkListener, error) {
	if err := ensureSocketDir(n.socketDir); err != nil {
		return nil, err
	}
	lease, err := lockfile.Acquire(filepath.Join(n.socketDir, "game.lock"), 0)
	if err != nil {
		return nil, fmt.Errorf("streamnet: acquire endpoint lease: %w", err)
	}
	releaseOnError := true
	defer func() {
		if releaseOnError {
			_ = lease.Close()
		}
	}()

	var (
		inner   net.Listener
		cleanup func() error
	)
	if runtime.GOOS == "windows" {
		if err = preparePublishedAddress(n.socketDir); err == nil {
			inner, err = net.Listen("tcp", "127.0.0.1:0")
		}
		if err == nil {
			address := inner.Addr().String()
			var path string
			path, err = publishAddress(n.socketDir, address)
			cleanup = func() error { return removePublishedAddress(path, address) }
		}
	} else {
		path := filepath.Join(n.socketDir, unixEndpointName)
		if err = prepareUnixEndpoint(path); err == nil {
			inner, err = net.Listen("unix", path)
		}
		if err == nil {
			unix, ok := inner.(*net.UnixListener)
			if !ok {
				_ = inner.Close()
				return nil, fmt.Errorf("streamnet: Unix listener has type %T", inner)
			}
			unix.SetUnlinkOnClose(false)
			identity, identityErr := unixEndpointIdentityAt(path)
			if identityErr != nil {
				_ = inner.Close()
				return nil, identityErr
			}
			if chmodErr := os.Chmod(path, 0o600); chmodErr != nil {
				_ = inner.Close()
				_ = removeUnixEndpoint(path, identity)
				return nil, fmt.Errorf("streamnet: secure Unix endpoint: %w", chmodErr)
			}
			cleanup = func() error { return removeUnixEndpoint(path, identity) }
		}
	}
	if err != nil {
		if inner != nil {
			_ = inner.Close()
		}
		return nil, err
	}

	result := &listener{
		Listener:    inner,
		id:          randomListenerID(),
		cleanup:     cleanup,
		lease:       lease,
		connections: make(map[*FramedConn]struct{}),
	}
	releaseOnError = false
	return result, nil
}

type listener struct {
	net.Listener
	id          int64
	cleanup     func() error
	lease       io.Closer
	once        sync.Once
	err         error
	mu          sync.Mutex
	closed      bool
	connections map[*FramedConn]struct{}
}

func (l *listener) Accept() (net.Conn, error) {
	conn, err := l.Listener.Accept()
	if err != nil {
		return nil, err
	}
	var framed *FramedConn
	framed = newTrackedFramedConn(conn, func() { l.removeConnection(framed) })
	l.mu.Lock()
	if l.closed {
		l.mu.Unlock()
		_ = framed.Close()
		return nil, net.ErrClosed
	}
	l.connections[framed] = struct{}{}
	l.mu.Unlock()
	return framed, nil
}

func (l *listener) removeConnection(conn *FramedConn) {
	l.mu.Lock()
	delete(l.connections, conn)
	l.mu.Unlock()
}

func (l *listener) ID() int64 { return l.id }

func (l *listener) PongData([]byte) {}

func (l *listener) Close() error {
	l.once.Do(func() {
		l.mu.Lock()
		l.closed = true
		connections := make([]*FramedConn, 0, len(l.connections))
		for conn := range l.connections {
			connections = append(connections, conn)
		}
		l.connections = make(map[*FramedConn]struct{})
		l.mu.Unlock()

		closeErr := l.Listener.Close()
		connectionErrors := make([]error, 0, len(connections))
		for _, conn := range connections {
			connectionErrors = append(connectionErrors, conn.Close())
		}
		cleanupErr := l.cleanup()
		leaseErr := l.lease.Close()
		l.err = errors.Join(closeErr, errors.Join(connectionErrors...), cleanupErr, leaseErr)
	})
	return l.err
}

func randomListenerID() int64 {
	var bytes [8]byte
	if _, err := cryptorand.Read(bytes[:]); err != nil {
		return 1
	}
	id := int64(binary.LittleEndian.Uint64(bytes[:]) & (1<<63 - 1))
	if id == 0 {
		return 1
	}
	return id
}
