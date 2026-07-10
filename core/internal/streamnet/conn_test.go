package streamnet

import (
	"bytes"
	"encoding/binary"
	"errors"
	"io"
	"net"
	"testing"
	"time"
)

func TestFrameOneByte(t *testing.T) {
	server, client := net.Pipe()
	defer server.Close()
	defer client.Close()

	go func() {
		_, _ = client.Write([]byte{0, 0, 0, 1, 0xfe})
	}()

	got, err := NewFramedConn(server).ReadPacket()
	if err != nil {
		t.Fatalf("ReadPacket() error = %v", err)
	}
	if len(got) != 1 || got[0] != 0xfe {
		t.Fatalf("ReadPacket() = %x, want fe", got)
	}
}

func TestFrameMaximumLengthDecisionDoesNotAllocate(t *testing.T) {
	if err := validateFrameLength(maxFrameLen); err != nil {
		t.Fatalf("validateFrameLength(maxFrameLen) error = %v", err)
	}
	if err := validateFrameLength(maxFrameLen + 1); !errors.Is(err, ErrFrameTooLarge) {
		t.Fatalf("validateFrameLength(maxFrameLen+1) error = %v, want ErrFrameTooLarge", err)
	}
}

func TestFrameFIFO(t *testing.T) {
	server, client := net.Pipe()
	defer server.Close()
	defer client.Close()

	writer := NewFramedConn(client)
	reader := NewFramedConn(server)
	want := [][]byte{{0xfe, 1}, {0xfe, 2}, {0xfe, 3}}
	errC := make(chan error, 1)
	go func() {
		for _, frame := range want {
			if _, err := writer.Write(frame); err != nil {
				errC <- err
				return
			}
		}
		errC <- nil
	}()

	for i, frame := range want {
		got, err := reader.ReadPacket()
		if err != nil {
			t.Fatalf("ReadPacket(%d) error = %v", i, err)
		}
		if string(got) != string(frame) {
			t.Fatalf("ReadPacket(%d) = %x, want %x", i, got, frame)
		}
	}
	if err := <-errC; err != nil {
		t.Fatalf("Write() error = %v", err)
	}
}

func TestFramePartialHeaderEOF(t *testing.T) {
	conn := NewFramedConn(&readConn{Reader: bytes.NewReader([]byte{0, 0})})
	_, err := conn.ReadPacket()
	if !errors.Is(err, io.ErrUnexpectedEOF) {
		t.Fatalf("ReadPacket() error = %v, want io.ErrUnexpectedEOF", err)
	}
	if !errors.Is(err, net.ErrClosed) {
		t.Fatalf("ReadPacket() error = %v, want terminal net.ErrClosed classification", err)
	}
}

func TestFramePartialPayloadEOF(t *testing.T) {
	conn := NewFramedConn(&readConn{Reader: bytes.NewReader([]byte{0, 0, 0, 3, 0xfe})})
	_, err := conn.ReadPacket()
	if !errors.Is(err, io.ErrUnexpectedEOF) {
		t.Fatalf("ReadPacket() error = %v, want io.ErrUnexpectedEOF", err)
	}
	if !errors.Is(err, net.ErrClosed) {
		t.Fatalf("ReadPacket() error = %v, want terminal net.ErrClosed classification", err)
	}
}

func TestFrameCleanEOFBetweenFrames(t *testing.T) {
	conn := NewFramedConn(&readConn{Reader: bytes.NewReader(nil)})
	_, err := conn.ReadPacket()
	if !errors.Is(err, io.EOF) {
		t.Fatalf("ReadPacket() error = %v, want io.EOF", err)
	}
	if !errors.Is(err, net.ErrClosed) {
		t.Fatalf("ReadPacket() error = %v, want terminal net.ErrClosed classification", err)
	}
}

func TestFrameZeroLength(t *testing.T) {
	conn := NewFramedConn(&readConn{Reader: bytes.NewReader([]byte{0, 0, 0, 0})})
	_, err := conn.ReadPacket()
	if !errors.Is(err, ErrInvalidFrameLength) {
		t.Fatalf("ReadPacket() error = %v, want ErrInvalidFrameLength", err)
	}
}

func TestFrameOversizedLength(t *testing.T) {
	var header [4]byte
	binary.BigEndian.PutUint32(header[:], uint32(maxFrameLen+1))
	conn := NewFramedConn(&readConn{Reader: bytes.NewReader(header[:])})
	_, err := conn.ReadPacket()
	if !errors.Is(err, ErrFrameTooLarge) {
		t.Fatalf("ReadPacket() error = %v, want ErrFrameTooLarge", err)
	}
}

func TestFrameWriteDeadlinePropagation(t *testing.T) {
	server, client := net.Pipe()
	defer server.Close()
	defer client.Close()

	conn := NewFramedConn(client)
	if err := conn.SetWriteDeadline(time.Now().Add(50 * time.Millisecond)); err != nil {
		t.Fatalf("SetWriteDeadline() error = %v", err)
	}
	_, err := conn.Write([]byte{0xfe})
	var netErr net.Error
	if !errors.As(err, &netErr) || !netErr.Timeout() {
		t.Fatalf("Write() error = %v, want timeout net.Error", err)
	}
}

func TestFrameWriteNormalizesTerminalPeerError(t *testing.T) {
	conn := NewFramedConn(&readConn{Reader: bytes.NewReader(nil)})
	_, err := conn.Write([]byte{0xfe})
	if !errors.Is(err, net.ErrClosed) {
		t.Fatalf("Write() error = %v, want net.ErrClosed classification", err)
	}
	if !errors.Is(err, io.ErrClosedPipe) {
		t.Fatalf("Write() error = %v, want original io.ErrClosedPipe cause", err)
	}
}

func TestFrameWritePreservesTemporaryNonTimeoutError(t *testing.T) {
	wantErr := temporaryError{}
	conn := NewFramedConn(&writeErrorConn{
		readConn: readConn{Reader: bytes.NewReader(nil)},
		err:      wantErr,
	})
	_, err := conn.Write([]byte{0xfe})
	if !errors.Is(err, wantErr) {
		t.Fatalf("Write() error = %v, want original temporary error", err)
	}
	if errors.Is(err, net.ErrClosed) {
		t.Fatalf("Write() error = %v, must not be classified net.ErrClosed", err)
	}
}

func TestFrameWriteDoesNotHideMixedTerminalError(t *testing.T) {
	wantErr := errors.New("application write failure")
	conn := NewFramedConn(&writeErrorConn{
		readConn: readConn{Reader: bytes.NewReader(nil)},
		err:      errors.Join(wantErr, io.ErrClosedPipe),
	})
	_, err := conn.Write([]byte{0xfe})
	if !errors.Is(err, wantErr) || !errors.Is(err, io.ErrClosedPipe) {
		t.Fatalf("Write() error = %v, want both original causes", err)
	}
	if errors.Is(err, net.ErrClosed) {
		t.Fatalf("Write() error = %v, mixed failure must not be classified net.ErrClosed", err)
	}
}

func TestFrameBlockedPeerCancellation(t *testing.T) {
	server, client := net.Pipe()
	defer client.Close()
	conn := NewFramedConn(client)

	errC := make(chan error, 1)
	go func() {
		_, err := conn.Write([]byte{0xfe})
		errC <- err
	}()
	time.Sleep(20 * time.Millisecond)
	_ = server.Close()

	select {
	case err := <-errC:
		if err == nil {
			t.Fatal("Write() error = nil after peer close")
		}
	case <-time.After(time.Second):
		t.Fatal("Write() remained blocked after peer close")
	}
}

func TestFrameWriteBytes(t *testing.T) {
	server, client := net.Pipe()
	defer server.Close()
	defer client.Close()

	errC := make(chan error, 1)
	go func() {
		_, err := NewFramedConn(client).Write([]byte{0xfe, 0x01})
		errC <- err
	}()
	got := make([]byte, 6)
	if _, err := io.ReadFull(server, got); err != nil {
		t.Fatalf("ReadFull() error = %v", err)
	}
	want := []byte{0, 0, 0, 2, 0xfe, 0x01}
	if string(got) != string(want) {
		t.Fatalf("wire bytes = %x, want %x", got, want)
	}
	if err := <-errC; err != nil {
		t.Fatalf("Write() error = %v", err)
	}
}

type readConn struct {
	io.Reader
}

type writeErrorConn struct {
	readConn
	err error
}

func (c *writeErrorConn) Write([]byte) (int, error) { return 0, c.err }

type temporaryError struct{}

func (temporaryError) Error() string   { return "temporary transport failure" }
func (temporaryError) Timeout() bool   { return false }
func (temporaryError) Temporary() bool { return true }

func (c *readConn) Write([]byte) (int, error)        { return 0, io.ErrClosedPipe }
func (c *readConn) Close() error                     { return nil }
func (c *readConn) LocalAddr() net.Addr              { return testAddr("local") }
func (c *readConn) RemoteAddr() net.Addr             { return testAddr("remote") }
func (c *readConn) SetDeadline(time.Time) error      { return nil }
func (c *readConn) SetReadDeadline(time.Time) error  { return nil }
func (c *readConn) SetWriteDeadline(time.Time) error { return nil }

type testAddr string

func (a testAddr) Network() string { return "test" }
func (a testAddr) String() string  { return string(a) }
