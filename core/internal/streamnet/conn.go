// Package streamnet adapts a local byte-stream connection to gophertunnel's
// packet-oriented transport contract.
package streamnet

import (
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"net"
	"sync"
)

const (
	// MaxFrameLen is the largest local transport frame accepted by the bridge.
	MaxFrameLen = 64 * 1024 * 1024
	maxFrameLen = MaxFrameLen
)

var (
	// ErrInvalidFrameLength is returned for an empty local transport frame.
	ErrInvalidFrameLength = errors.New("streamnet: frame length must be positive")
	// ErrFrameTooLarge is returned when a local transport frame exceeds MaxFrameLen.
	ErrFrameTooLarge = errors.New("streamnet: frame exceeds 64 MiB")
)

// FramedConn wraps a net.Conn with an unsigned 32-bit big-endian length prefix.
// Each Write is one complete frame and ReadPacket reads one complete frame.
type FramedConn struct {
	net.Conn
	writeMu sync.Mutex
}

// NewFramedConn wraps conn in the local bridge framing contract.
func NewFramedConn(conn net.Conn) *FramedConn {
	return &FramedConn{Conn: conn}
}

// ReadPacket reads exactly one framed payload. A clean EOF is only returned
// when it occurs between frames.
func (c *FramedConn) ReadPacket() ([]byte, error) {
	var header [4]byte
	if _, err := io.ReadFull(c.Conn, header[:]); err != nil {
		if errors.Is(err, io.EOF) {
			err = errors.Join(net.ErrClosed, err)
		}
		return nil, fmt.Errorf("streamnet: read frame header: %w", err)
	}
	length := binary.BigEndian.Uint32(header[:])
	if err := validateFrameLength64(uint64(length)); err != nil {
		return nil, err
	}
	payload := make([]byte, int(length))
	if _, err := io.ReadFull(c.Conn, payload); err != nil {
		return nil, fmt.Errorf("streamnet: read frame payload: %w", err)
	}
	return payload, nil
}

// Write writes b as one complete framed payload. Concurrent calls remain
// frame-atomic and preserve mutex acquisition order.
func (c *FramedConn) Write(b []byte) (int, error) {
	if err := validateFrameLength(len(b)); err != nil {
		return 0, err
	}

	c.writeMu.Lock()
	defer c.writeMu.Unlock()

	var header [4]byte
	binary.BigEndian.PutUint32(header[:], uint32(len(b)))
	if _, err := writeFull(c.Conn, header[:]); err != nil {
		return 0, fmt.Errorf("streamnet: write frame header: %w", normalizeWriteError(err))
	}
	n, err := writeFull(c.Conn, b)
	if err != nil {
		return n, fmt.Errorf("streamnet: write frame payload: %w", normalizeWriteError(err))
	}
	return n, nil
}

func normalizeWriteError(err error) error {
	var netErr net.Error
	if errors.As(err, &netErr) && netErr.Timeout() {
		return err
	}
	return errors.Join(net.ErrClosed, err)
}

func validateFrameLength(length int) error {
	if length < 0 {
		return ErrInvalidFrameLength
	}
	return validateFrameLength64(uint64(length))
}

func validateFrameLength64(length uint64) error {
	if length == 0 {
		return ErrInvalidFrameLength
	}
	if length > MaxFrameLen {
		return fmt.Errorf("%w: got %d bytes, maximum is %d", ErrFrameTooLarge, length, MaxFrameLen)
	}
	return nil
}

func writeFull(w io.Writer, p []byte) (int, error) {
	written := 0
	for written < len(p) {
		n, err := w.Write(p[written:])
		written += n
		if err != nil {
			return written, err
		}
		if n == 0 {
			return written, io.ErrNoProgress
		}
	}
	return written, nil
}
