package core_test

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/binary"
	"errors"
	"io"
	"net"
	"testing"
	"time"

	"github.com/sandertv/go-raknet"
)

// TestTransportClientGUIDsUseFreshEntropyAndVariantBits checks the effective transport dependency.
func TestTransportClientGUIDsUseFreshEntropyAndVariantBits(t *testing.T) {
	entropy := bytes.NewReader(append(bytes.Repeat([]byte{0xff}, 8), make([]byte, 8)...))
	previous := rand.Reader
	rand.Reader = entropy
	t.Cleanup(func() { rand.Reader = previous })
	for _, want := range []uint64{0xbfffffffffffffff, 0x8000000000000000} {
		conn := &guidProbeConn{}
		dialer := raknet.Dialer{UpstreamDialer: guidProbeDialer{conn}}
		if _, err := dialer.PingContext(context.Background(), "127.0.0.1:19132"); !errors.Is(err, io.EOF) {
			t.Fatalf("synthetic end of ping: %v", err)
		}
		if !conn.closed {
			t.Fatal("ping left its socket open")
		}
		if len(conn.sent) < 8 {
			t.Fatalf("ping has no client GUID: %d bytes", len(conn.sent))
		}
		got := binary.BigEndian.Uint64(conn.sent[len(conn.sent)-8:])
		if got != want {
			t.Fatalf("client GUID=%016x, want fresh entropy with variant bits=%016x", got, want)
		}
	}
}

type guidProbeDialer struct{ conn *guidProbeConn }

// DialContext returns an isolated byte recorder without opening a network socket.
func (d guidProbeDialer) DialContext(context.Context, string, string) (net.Conn, error) {
	return d.conn, nil
}

type guidProbeConn struct {
	sent   []byte
	closed bool
}

// Write retains the actual ping produced by the selected dependency.
func (c *guidProbeConn) Write(data []byte) (int, error) {
	c.sent = append([]byte(nil), data...)
	return len(data), nil
}

// Read ends the synthetic exchange after its outgoing bytes are available.
func (*guidProbeConn) Read([]byte) (int, error) { return 0, io.EOF }

// Close records that the dialer released its transport.
func (c *guidProbeConn) Close() error { c.closed = true; return nil }

// LocalAddr supplies the socket identity expected by the dialer.
func (*guidProbeConn) LocalAddr() net.Addr { return &net.UDPAddr{} }

// RemoteAddr supplies a synthetic peer identity.
func (*guidProbeConn) RemoteAddr() net.Addr { return &net.UDPAddr{} }

// SetDeadline accepts the dialer's deadline without using a wall clock.
func (*guidProbeConn) SetDeadline(time.Time) error { return nil }

// SetReadDeadline accepts the dialer's read deadline without scheduling work.
func (*guidProbeConn) SetReadDeadline(time.Time) error { return nil }

// SetWriteDeadline accepts the dialer's write deadline without scheduling work.
func (*guidProbeConn) SetWriteDeadline(time.Time) error { return nil }
