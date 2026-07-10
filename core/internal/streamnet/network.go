package streamnet

import (
	"context"
	cryptorand "crypto/rand"
	"encoding/binary"
	"errors"
	"fmt"
	"net"
	"os"
	"path/filepath"
	"runtime"
	"sync"

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

	var (
		inner   net.Listener
		cleanup func() error
		err     error
	)
	if runtime.GOOS == "windows" {
		inner, err = net.Listen("tcp", "127.0.0.1:0")
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
			if chmodErr := os.Chmod(path, 0o600); chmodErr != nil {
				_ = inner.Close()
				_ = removeUnixEndpoint(path)
				return nil, fmt.Errorf("streamnet: secure Unix endpoint: %w", chmodErr)
			}
			cleanup = func() error { return removeUnixEndpoint(path) }
		}
	}
	if err != nil {
		if inner != nil {
			_ = inner.Close()
		}
		return nil, err
	}

	return &listener{
		Listener: inner,
		id:       randomListenerID(),
		cleanup:  cleanup,
	}, nil
}

type listener struct {
	net.Listener
	id      int64
	cleanup func() error
	once    sync.Once
	err     error
}

func (l *listener) Accept() (net.Conn, error) {
	conn, err := l.Listener.Accept()
	if err != nil {
		return nil, err
	}
	return NewFramedConn(conn), nil
}

func (l *listener) ID() int64 { return l.id }

func (l *listener) PongData([]byte) {}

func (l *listener) Close() error {
	l.once.Do(func() {
		closeErr := l.Listener.Close()
		cleanupErr := l.cleanup()
		l.err = errors.Join(closeErr, cleanupErr)
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
