package main

import (
	"bytes"
	"testing"

	"github.com/go-gl/mathgl/mgl32"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// TestPrimitiveGalleryRoundtrip verifies the fixture sends the wire format the client decodes.
func TestPrimitiveGalleryRoundtrip(t *testing.T) {
	expected := primitiveGallery(mgl32.Vec3{1, 2, 3}, 57)
	var buffer bytes.Buffer
	(&packet.PrimitiveShapes{Shapes: expected}).Marshal(protocol.NewWriter(&buffer, 0))
	var decoded packet.PrimitiveShapes
	decoded.Marshal(protocol.NewReader(&buffer, 0, true))
	if len(decoded.Shapes) != len(expected) {
		t.Fatalf("got %d shapes, want %d", len(decoded.Shapes), len(expected))
	}
	for index, shape := range expected {
		if !shape.Equal(decoded.Shapes[index]) {
			t.Fatalf("shape %d changed during wire roundtrip", index)
		}
	}
}
