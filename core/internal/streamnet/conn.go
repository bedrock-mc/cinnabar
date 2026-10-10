// Package streamnet provides local control and framed session transports.
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
	writeMu   sync.Mutex
	closeOnce sync.Once
	closeErr  error

	aheadMu  sync.Mutex
	ahead    chan framedRead // the read-ahead frame ReadPacket must return next
	peerDone chan struct{}
}

type framedRead struct {
	payload []byte
	err     error
}

// NewFramedConn wraps conn in the local bridge framing contract.
func NewFramedConn(conn net.Conn) *FramedConn {
	return &FramedConn{Conn: conn}
}

// PeerDone reads the next frame ahead and returns a channel closed if that read ends the
// connection; ReadPacket still returns the frame or error in order. Call it only while no
// ReadPacket is in flight.
func (c *FramedConn) PeerDone() <-chan struct{} {
	c.aheadMu.Lock()
	defer c.aheadMu.Unlock()
	if c.peerDone != nil {
		return c.peerDone
	}
	done, ahead := make(chan struct{}), make(chan framedRead, 1)
	c.peerDone, c.ahead = done, ahead
	go func() {
		payload, err := c.readFrame()
		if err != nil {
			close(done)
		}
		ahead <- framedRead{payload: payload, err: err}
	}()
	return done
}

// Close closes the underlying transport once.
func (c *FramedConn) Close() error {
	c.closeOnce.Do(func() {
		c.closeErr = c.Conn.Close()
	})
	return c.closeErr
}

// ReadPacket reads exactly one framed payload. A clean EOF is only returned
// when it occurs between frames.
func (c *FramedConn) ReadPacket() ([]byte, error) {
	c.aheadMu.Lock()
	ahead := c.ahead
	c.ahead, c.peerDone = nil, nil
	c.aheadMu.Unlock()
	if ahead != nil {
		read := <-ahead
		return read.payload, read.err
	}
	return c.readFrame()
}

func (c *FramedConn) readFrame() ([]byte, error) {
	var header [4]byte
	if _, err := io.ReadFull(c.Conn, header[:]); err != nil {
		return nil, fmt.Errorf("streamnet: read frame header: %w", classifyTerminalError(err))
	}
	length := binary.BigEndian.Uint32(header[:])
	if err := validateFrameLength64(uint64(length)); err != nil {
		return nil, err
	}
	payload := make([]byte, int(length))
	if _, err := io.ReadFull(c.Conn, payload); err != nil {
		if errors.Is(err, io.EOF) {
			err = io.ErrUnexpectedEOF
		}
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
	if vectoredConn(c.Conn) {
		buffers := net.Buffers{header[:], b}
		written, err := buffers.WriteTo(c.Conn)
		if err == nil && written < int64(len(header)+len(b)) {
			// Windows sends net.Buffers with one WSASend that may complete short.
			written, err = finishFrame(c.Conn, header[:], b, written)
		}
		if err == nil {
			return len(b), nil
		}
		if written < int64(len(header)) {
			return 0, fmt.Errorf("streamnet: write frame header: %w", classifyTerminalError(err))
		}
		return int(written) - len(header), fmt.Errorf("streamnet: write frame payload: %w", classifyTerminalError(err))
	}
	if _, err := writeFull(c.Conn, header[:]); err != nil {
		return 0, fmt.Errorf("streamnet: write frame header: %w", classifyTerminalError(err))
	}
	n, err := writeFull(c.Conn, b)
	if err != nil {
		return n, fmt.Errorf("streamnet: write frame payload: %w", classifyTerminalError(err))
	}
	return n, nil
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

// vectoredConn reports conns whose net.Buffers path writes the frame in one
// vectored call; a short completion is finished by the caller.
func vectoredConn(conn net.Conn) bool {
	switch conn.(type) {
	case *net.UnixConn, *net.TCPConn:
		return true
	default:
		return false
	}
}

// finishFrame writes whatever part of header+payload a vectored write left and
// returns the total bytes written.
func finishFrame(w io.Writer, header, payload []byte, written int64) (int64, error) {
	if written < int64(len(header)) {
		n, err := writeFull(w, header[written:])
		written += int64(n)
		if err != nil {
			return written, err
		}
	}
	n, err := writeFull(w, payload[written-int64(len(header)):])
	return written + int64(n), err
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
