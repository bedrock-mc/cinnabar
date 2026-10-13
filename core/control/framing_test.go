package control

import (
	"bytes"
	"encoding/binary"
	"strings"
	"testing"
)

func TestResponseFramePayloadBoundary(t *testing.T) {
	for _, extra := range []int{0, 1} {
		name := "at limit"
		if extra != 0 {
			name = "over limit"
		}
		t.Run(name, func(t *testing.T) {
			var wire bytes.Buffer
			// JSON quotes count toward the payload limit; the length header does not.
			err := writeResponse(&wire, strings.Repeat("x", MaxFrameLen-2+extra))
			if extra != 0 {
				if err == nil || wire.Len() != 0 {
					t.Fatalf("oversized response: error %v, wrote %d bytes", err, wire.Len())
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			if wire.Len() != MaxFrameLen+4 || binary.BigEndian.Uint32(wire.Bytes()[:4]) != MaxFrameLen {
				t.Fatalf("frame length = %d", wire.Len())
			}
			payload, err := readFrame(&wire)
			if err != nil || len(payload) != MaxFrameLen || wire.Len() != 0 {
				t.Fatalf("read frame: payload %d bytes, remaining %d, error %v", len(payload), wire.Len(), err)
			}
		})
	}
}
