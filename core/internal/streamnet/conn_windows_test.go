//go:build windows

package streamnet

import (
	"bytes"
	"errors"
	"net"
	"syscall"
	"testing"
)

func TestFrameWriteClassifiesWindowsPeerAbort(t *testing.T) {
	wantErr := syscall.WSAECONNABORTED
	conn := NewFramedConn(&writeErrorConn{
		readConn: readConn{Reader: bytes.NewReader(nil)},
		err: &net.OpError{
			Op:  "write",
			Net: "tcp",
			Err: wantErr,
		},
	})
	_, err := conn.Write([]byte{0xfe})
	if !errors.Is(err, wantErr) || !errors.Is(err, net.ErrClosed) {
		t.Fatalf("Write() error = %v, want WSAECONNABORTED plus net.ErrClosed", err)
	}
}
