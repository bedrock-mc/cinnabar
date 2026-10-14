package webrelay

import "io"

// inboundFrameChunkBytes bounds the initial allocation for an unfulfilled remote frame header.
const inboundFrameChunkBytes = 32 << 10

// ReadFramePayload grows with received data rather than trusting the remote frame's advertised
// length. Reading through conn retains upload pacing and cancellation on both transports.
func (conn *uploadLimitedConn) ReadFramePayload(size int) ([]byte, error) {
	if size <= inboundFrameChunkBytes {
		payload := make([]byte, size)
		_, err := io.ReadFull(conn, payload)
		return payload, err
	}
	var chunk [inboundFrameChunkBytes]byte
	var payload []byte
	for len(payload) < size {
		n, err := io.ReadFull(conn, chunk[:min(len(chunk), size-len(payload))])
		if err != nil {
			return nil, err
		}
		payload = append(payload, chunk[:n]...)
	}
	return payload, nil
}
