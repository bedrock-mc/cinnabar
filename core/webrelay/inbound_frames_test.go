package webrelay

import (
	"bytes"
	"encoding/binary"
	"errors"
	"io"
	"net"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"golang.org/x/time/rate"
)

// observedFrameConn records the buffer capacity exposed to a remote payload read.
type observedFrameConn struct {
	net.Conn
	reader      io.Reader
	maxCapacity int
	readLimit   int
}

// Read records allocation exposure while returning the fixture's framed byte stream.
func (conn *observedFrameConn) Read(data []byte) (int, error) {
	conn.maxCapacity = max(conn.maxCapacity, cap(data))
	if conn.readLimit > 0 {
		data = data[:min(len(data), conn.readLimit)]
	}
	return conn.reader.Read(data)
}

func TestRemoteFrameHeaderDoesNotAllocateAdvertisedPayload(t *testing.T) {
	var header [4]byte
	binary.BigEndian.PutUint32(header[:], streamnet.MaxFrameLen)
	transport := &observedFrameConn{reader: bytes.NewReader(header[:])}
	conn := &uploadLimitedConn{Conn: transport, ctx: t.Context(), limiter: rate.NewLimiter(rate.Inf, playMessageLimit)}
	if _, err := streamnet.NewFramedConn(conn).ReadPacket(); !errors.Is(err, io.ErrUnexpectedEOF) {
		t.Fatal("unfulfilled payload did not fail as truncated", err)
	}
	if transport.maxCapacity > inboundFrameChunkBytes {
		t.Fatalf("header alone exposed a %d-byte allocation to the remote read", transport.maxCapacity)
	}
}

func TestRemoteFramesPreserveLargePayloadsAndFraming(t *testing.T) {
	payload := bytes.Repeat([]byte{0xa5}, playMessageLimit+1)
	var encoded bytes.Buffer
	for _, frame := range [][]byte{payload, {0xfe, 42}} {
		var header [4]byte
		binary.BigEndian.PutUint32(header[:], uint32(len(frame)))
		encoded.Write(header[:])
		encoded.Write(frame)
	}
	transport := &observedFrameConn{reader: bytes.NewReader(encoded.Bytes()), readLimit: inboundFrameChunkBytes - 1}
	conn := &uploadLimitedConn{Conn: transport, ctx: t.Context(), limiter: rate.NewLimiter(rate.Inf, playMessageLimit)}
	framed := streamnet.NewFramedConn(conn)
	for _, want := range [][]byte{payload, {0xfe, 42}} {
		got, err := framed.ReadPacket()
		if err != nil || !bytes.Equal(got, want) {
			t.Fatal("incremental buffering changed a valid frame", err)
		}
	}
	if _, err := framed.ReadPacket(); !errors.Is(err, io.EOF) {
		t.Fatal("frame boundary EOF changed", err)
	}
	if transport.maxCapacity > inboundFrameChunkBytes {
		t.Fatal("a large payload bypassed bounded incremental reads")
	}
}
