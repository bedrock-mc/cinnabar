// Package hostbridge connects an authorized native peer to this process's Dragonfly world.
// It never authenticates Xbox users or opens a Minecraft client connection.
package hostbridge

import (
	"encoding/binary"
	"encoding/json"
	"errors"
	"io"
	"net"
	"sync"
	"time"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/internal/hostcontract"
)

var errProtocol = errors.New("invalid native world bridge state")

// wire serializes complete frames, never holding its lock while awaiting a remote receipt.
type wire struct {
	net.Conn
	writeMu      sync.Mutex
	baseDeadline time.Time
}

// read validates the original header and kind before allocating its bounded body.
func (w *wire) read() (hostcontract.Kind, []byte, error) {
	var header [5]byte
	if _, err := io.ReadFull(w.Conn, header[:]); err != nil {
		return 0, nil, err
	}
	length := binary.BigEndian.Uint32(header[:4])
	kind := hostcontract.Kind(header[4])
	if length < 1 || kind.Maximum() == 0 || length-1 > kind.Maximum() {
		return 0, nil, errProtocol
	}
	body := make([]byte, length-1)
	_, err := io.ReadFull(w.Conn, body)
	return kind, body, err
}

// write submits one bounded frame in order without logging any original payload.
func (w *wire) write(kind hostcontract.Kind, body []byte) error {
	return w.writeBefore(kind, body, time.Now().Add(time.Duration(hostcontract.CLOSE_MILLIS)*time.Millisecond))
}

// writeBefore retains one operation's finite deadline across all partial writes.
func (w *wire) writeBefore(kind hostcontract.Kind, body []byte, deadline time.Time) error {
	if kind.Maximum() == 0 || uint64(len(body)) > uint64(kind.Maximum()) {
		return errProtocol
	}
	var header [5]byte
	binary.BigEndian.PutUint32(header[:4], uint32(len(body))+1)
	header[4] = byte(kind)
	w.writeMu.Lock()
	defer w.writeMu.Unlock()
	if !w.baseDeadline.IsZero() && w.baseDeadline.Before(deadline) {
		deadline = w.baseDeadline
	}
	if err := w.Conn.SetWriteDeadline(deadline); err != nil {
		return err
	}
	if err := writeAll(w.Conn, header[:]); err != nil {
		return err
	}
	return writeAll(w.Conn, body)
}

// setBase changes the original establishment cutoff only at actual initialization.
func (w *wire) setBase(deadline time.Time) error {
	w.writeMu.Lock()
	defer w.writeMu.Unlock()
	w.baseDeadline = deadline
	return w.Conn.SetReadDeadline(deadline)
}

// writeAll rejects a broken writer that reports no progress.
func writeAll(w io.Writer, b []byte) error {
	for len(b) > 0 {
		n, err := w.Write(b)
		if err != nil {
			return err
		}
		if n <= 0 || n > len(b) {
			return io.ErrShortWrite
		}
		b = b[n:]
	}
	return nil
}

// json writes one generated record through the same framing primitive.
func (w *wire) json(kind hostcontract.Kind, value any) error {
	b, err := json.Marshal(value)
	if err != nil {
		return errProtocol
	}
	return w.write(kind, b)
}

// decode rejects malformed control envelopes while leaving raw client data opaque.
func decode(b []byte, value any) error {
	if !json.Valid(b) {
		return errProtocol
	}
	if err := json.Unmarshal(b, value); err != nil {
		return errProtocol
	}
	return nil
}
