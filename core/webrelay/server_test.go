package webrelay

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/coder/websocket"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
)

func TestWebSocketSplitsLargeBridgeFramesWithoutChangingStream(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), helloTimeout)
	defer cancel()
	payload := bytes.Repeat([]byte{0xa5}, 2*playMessageLimit+1)
	done := make(chan error, 1)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		socket, err := websocket.Accept(w, r, nil)
		if err != nil {
			done <- err
			return
		}
		defer socket.CloseNow()
		conn := newSocketConn(ctx, socket, playMessageLimit)
		n, err := streamnet.NewFramedConn(conn).Write(payload)
		if err == nil && n != len(payload) {
			err = io.ErrShortWrite
		}
		done <- err
	}))
	defer server.Close()
	socket, _, err := websocket.Dial(ctx, "ws"+strings.TrimPrefix(server.URL, "http"), nil)
	if err != nil {
		t.Fatal(err)
	}
	defer socket.CloseNow()
	socket.SetReadLimit(playMessageLimit)
	var received bytes.Buffer
	for received.Len() < 4+len(payload) {
		kind, message, err := socket.Read(ctx)
		if err != nil {
			t.Fatal("valid bridge frame exceeded the WebSocket contract", err)
		}
		if kind != websocket.MessageBinary || len(message) > playMessageLimit {
			t.Fatal("outbound message violated the play contract")
		}
		received.Write(message)
	}
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	data := received.Bytes()
	if binary.BigEndian.Uint32(data[:4]) != uint32(len(payload)) || !bytes.Equal(data[4:], payload) {
		t.Fatal("message splitting changed the bridge frame")
	}
}

// partialSocketConn accepts one message before partially writing the next with the supplied error.
type partialSocketConn struct {
	net.Conn
	writes int
	err    error
}

// Write models a WebSocket write failure after earlier messages were sent successfully.
func (conn *partialSocketConn) Write(data []byte) (int, error) {
	conn.writes++
	if conn.writes == 1 {
		return len(data), nil
	}
	return 1, conn.err
}

func TestSocketWriteReportsPartialProgressAndStopsOnFailure(t *testing.T) {
	for _, cause := range []error{net.ErrClosed, nil} {
		transport := &partialSocketConn{err: cause}
		conn := &socketConn{Conn: transport, messageLimit: 2}
		n, err := conn.Write([]byte("abcdef"))
		want := cause
		if want == nil {
			want = io.ErrShortWrite
		}
		if n != 3 || !errors.Is(err, want) || transport.writes != 2 {
			t.Fatalf("write = (%d, %v), calls = %d; want partial progress and a terminal error", n, err, transport.writes)
		}
	}
}
