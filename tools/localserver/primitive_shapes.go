package main

import (
	"context"
	"image/color"
	"strings"
	"sync"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/go-gl/mathgl/mgl32"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// primitiveListener adds an opt-in packet fixture without changing ordinary local worlds.
func primitiveListener(inner func(server.Config) (server.Listener, error)) func(server.Config) (server.Listener, error) {
	return func(conf server.Config) (server.Listener, error) {
		listener, err := inner(conf)
		if err != nil {
			return nil, err
		}
		return &shapeListener{Listener: listener}, nil
	}
}

type shapeListener struct{ server.Listener }

// Accept keeps the underlying connection available for the listener's Disconnect contract.
func (l *shapeListener) Accept() (session.Conn, error) {
	conn, err := l.Listener.Accept()
	if err != nil {
		return nil, err
	}
	return &shapeConn{Conn: conn}, nil
}

// Disconnect unwraps only this fixture's connection before delegating to another listener.
func (l *shapeListener) Disconnect(conn session.Conn, reason string) error {
	if wrapped, ok := conn.(*shapeConn); ok {
		conn = wrapped.Conn
	}
	return l.Listener.Disconnect(conn, reason)
}

type shapeConn struct {
	session.Conn
	mu     sync.Mutex
	origin mgl32.Vec3
	actor  int64
}

// StartGameContext publishes the gallery after the normal login handshake has completed.
func (c *shapeConn) StartGameContext(ctx context.Context, data minecraft.GameData) error {
	if err := c.Conn.StartGameContext(ctx, data); err != nil {
		return err
	}
	c.mu.Lock()
	c.origin, c.actor = data.PlayerPosition, data.EntityUniqueID
	c.mu.Unlock()
	return c.WritePacket(&packet.PrimitiveShapes{Shapes: primitiveGallery(data.PlayerPosition, data.EntityUniqueID)})
}

// ReadPacket handles fixture commands locally; all other packets retain their normal ordering.
func (c *shapeConn) ReadPacket() (packet.Packet, error) {
	for {
		pk, err := c.Conn.ReadPacket()
		if err != nil {
			return pk, err
		}
		request, ok := pk.(*packet.CommandRequest)
		if !ok || (request.CommandLine != "/shapes" && !strings.HasPrefix(request.CommandLine, "/shapes ")) {
			return pk, nil
		}
		c.mu.Lock()
		origin, actor := c.origin, c.actor
		c.mu.Unlock()
		shapes := primitiveGallery(origin, actor)
		switch strings.TrimSpace(strings.TrimPrefix(request.CommandLine, "/shapes")) {
		case "clear":
			for index, shape := range shapes {
				shapes[index] = protocol.PrimitiveShape{NetworkID: shape.NetworkID, ExtraShapeData: &protocol.LastShape{}}
			}
		case "update":
			shapes = []protocol.PrimitiveShape{{NetworkID: 2, Type: protocol.Option(protocol.PrimitiveShapeBox), Colour: protocol.Option(color.RGBA{R: 255, B: 255, A: 255}), ExtraShapeData: &protocol.LastShape{}}}
		}
		if err := c.WritePacket(&packet.PrimitiveShapes{Shapes: shapes}); err != nil {
			return nil, err
		}
	}
}

// primitiveGallery covers all six kinds, a dimension-hidden shape, and a player attachment.
func primitiveGallery(origin mgl32.Vec3, actor int64) []protocol.PrimitiveShape {
	shapes := make([]protocol.PrimitiveShape, 0, 8)
	kinds := []uint8{protocol.PrimitiveShapeLine, protocol.PrimitiveShapeBox, protocol.PrimitiveShapeSphere, protocol.PrimitiveShapeCircle, protocol.PrimitiveShapeArrow, protocol.PrimitiveShapeText}
	for index, kind := range kinds {
		p := origin.Add(mgl32.Vec3{float32(index%3)*4 - 4, 1 + float32(index/3)*3, 10})
		var data protocol.ShapeData = &protocol.LastShape{}
		switch kind {
		case protocol.PrimitiveShapeLine:
			data = &protocol.LineShape{LineEndLocation: p.Add(mgl32.Vec3{2, 2, 0})}
		case protocol.PrimitiveShapeBox:
			data = &protocol.BoxShape{BoxBound: mgl32.Vec3{2, 2, 2}}
		case protocol.PrimitiveShapeSphere, protocol.PrimitiveShapeCircle:
			data = &protocol.SphereShape{Segments: 20}
		case protocol.PrimitiveShapeArrow:
			data = &protocol.ArrowShape{ArrowEndLocation: protocol.Option(p.Add(mgl32.Vec3{2, 2, 0})), ArrowHeadLength: protocol.Option(float32(0.75)), ArrowHeadRadius: protocol.Option(float32(0.4)), Segments: protocol.Option(byte(8))}
		case protocol.PrimitiveShapeText:
			data = &protocol.TextShape{Text: "Primitive shapes\nRetained text", DepthTest: true, ShowBackface: true, ShowBackfaceText: true, BackgroundColour: protocol.Option(color.RGBA{A: 128})}
		}
		shapes = append(shapes, protocol.PrimitiveShape{NetworkID: uint64(index + 1), Type: protocol.Option(kind), Location: protocol.Option(p), Scale: protocol.Option(float32(1)), Colour: protocol.Option(color.RGBA{R: uint8(255 - index*25), G: uint8(60 + index*35), B: 100, A: 255}), DimensionID: protocol.Option(int32(0)), ExtraShapeData: data})
	}
	shapes = append(shapes, protocol.PrimitiveShape{NetworkID: 7, Type: protocol.Option(protocol.PrimitiveShapeBox), Location: protocol.Option(origin.Add(mgl32.Vec3{0, 0, 5})), DimensionID: protocol.Option(int32(1)), ExtraShapeData: &protocol.LastShape{}})
	shapes = append(shapes, protocol.PrimitiveShape{NetworkID: 8, Type: protocol.Option(protocol.PrimitiveShapeBox), Location: protocol.Option(mgl32.Vec3{0, 3, 0}), AttachedToEntityID: protocol.Option(actor), Colour: protocol.Option(color.RGBA{G: 255, B: 255, A: 255}), ExtraShapeData: &protocol.LastShape{}})
	return shapes
}
