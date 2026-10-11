package streamnet

import (
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"runtime"
	"sync"

	"github.com/hashimthearab/rust-mcbe/core/internal/lockfile"
)

// ListenControl opens the distinct raw control endpoint. The caller owns the
// accepted connections and must close them before closing the listener.
func ListenControl(socketDir string) (net.Listener, error) {
	inner, cleanup, lease, err := openEndpoint(socketDir, controlEndpoint)
	if err != nil {
		return nil, err
	}
	return &rawEndpointListener{Listener: inner, cleanup: cleanup, lease: lease}, nil
}

// ListenSession opens the distinct session endpoint, whose frames carry session messages
// rather than gophertunnel packets. The caller owns the accepted connections.
func ListenSession(socketDir string) (net.Listener, error) {
	inner, cleanup, lease, err := openEndpoint(socketDir, sessionEndpoint)
	if err != nil {
		return nil, err
	}
	return &rawEndpointListener{Listener: inner, cleanup: cleanup, lease: lease}, nil
}

func openEndpoint(socketDir string, names endpointNames) (net.Listener, func() error, io.Closer, error) {
	if err := ensureSocketDir(socketDir); err != nil {
		return nil, nil, nil, err
	}
	lease, err := lockfile.Acquire(filepath.Join(socketDir, names.lease), 0)
	if err != nil {
		return nil, nil, nil, fmt.Errorf("streamnet: acquire endpoint lease: %w", err)
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
		if err = preparePublishedAddressNamed(socketDir, names.windows); err == nil {
			inner, err = net.Listen("tcp", "127.0.0.1:0")
		}
		if err == nil {
			address := inner.Addr().String()
			var path string
			path, err = publishAddressNamed(socketDir, names.windows, address)
			cleanup = func() error { return removePublishedAddress(path, address) }
		}
	} else {
		path := unixEndpointPathNamed(socketDir, names.unix)
		if err = prepareUnixEndpoint(path); err == nil {
			inner, err = net.Listen("unix", path)
		}
		if err == nil {
			unix, ok := inner.(*net.UnixListener)
			if !ok {
				_ = inner.Close()
				return nil, nil, nil, fmt.Errorf("streamnet: Unix listener has type %T", inner)
			}
			unix.SetUnlinkOnClose(false)
			identity, identityErr := unixEndpointIdentityAt(path)
			if identityErr != nil {
				_ = inner.Close()
				return nil, nil, nil, identityErr
			}
			if chmodErr := os.Chmod(path, 0o600); chmodErr != nil {
				_ = inner.Close()
				_ = removeUnixEndpoint(path, identity)
				return nil, nil, nil, fmt.Errorf("streamnet: secure Unix endpoint: %w", chmodErr)
			}
			cleanup = func() error { return removeUnixEndpoint(path, identity) }
		}
	}
	if err != nil {
		if inner != nil {
			_ = inner.Close()
		}
		return nil, nil, nil, err
	}
	releaseOnError = false
	return inner, cleanup, lease, nil
}

// localSocketBufferBytes lets one large batch cross a Unix socket in a single write; macOS
// defaults to 8 KiB, which splits it and delays the frames queued behind it.
const localSocketBufferBytes = 4 << 20

// tuneLocalConn disables Nagle on loopback TCP and enlarges Unix socket buffers. It is best
// effort: an error returned from Accept would end gophertunnel's accept loop.
func tuneLocalConn(conn net.Conn) {
	switch c := conn.(type) {
	case *net.TCPConn:
		_ = c.SetNoDelay(true)
	case *net.UnixConn:
		_ = c.SetReadBuffer(localSocketBufferBytes)
		_ = c.SetWriteBuffer(localSocketBufferBytes)
	}
}

type rawEndpointListener struct {
	net.Listener
	cleanup func() error
	lease   io.Closer
	once    sync.Once
	err     error
}

func (listener *rawEndpointListener) Accept() (net.Conn, error) {
	conn, err := listener.Listener.Accept()
	if err != nil {
		return nil, err
	}
	tuneLocalConn(conn)
	return conn, nil
}

func (listener *rawEndpointListener) Close() error {
	listener.once.Do(func() {
		listener.err = errors.Join(listener.Listener.Close(), listener.cleanup(), listener.lease.Close())
	})
	return listener.err
}
