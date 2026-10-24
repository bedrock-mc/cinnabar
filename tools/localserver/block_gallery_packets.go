package main

import (
	"image/color"
	"strings"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl32"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

const galleryLabelFirstID = 8300000

// blockGalleryListener adds labels and raw block-actor data to the generated terrain.
func blockGalleryListener(inner func(server.Config) (server.Listener, error), gallery *blockGallery) func(server.Config) (server.Listener, error) {
	return func(conf server.Config) (server.Listener, error) {
		listener, err := inner(conf)
		if err != nil {
			return nil, err
		}
		return &blockGalleryPacketListener{Listener: listener, gallery: gallery}, nil
	}
}

type blockGalleryPacketListener struct {
	server.Listener
	gallery *blockGallery
}

// Accept wraps each connection without sharing mutable per-client state.
func (l *blockGalleryPacketListener) Accept() (session.Conn, error) {
	conn, err := l.Listener.Accept()
	if err != nil {
		return nil, err
	}
	return &blockGalleryConn{Conn: conn, gallery: l.gallery}, nil
}

// Disconnect returns the original connection to the listener that owns it.
func (l *blockGalleryPacketListener) Disconnect(conn session.Conn, reason string) error {
	if c, ok := conn.(*blockGalleryConn); ok {
		conn = c.Conn
	}
	return l.Listener.Disconnect(conn, reason)
}

type blockGalleryConn struct {
	session.Conn
	gallery *blockGallery
}

// WritePacket publishes labels with columns and actor defaults after their sub-chunks arrive.
func (c *blockGalleryConn) WritePacket(pk packet.Packet) error {
	if err := c.Conn.WritePacket(pk); err != nil {
		return err
	}
	switch pk := pk.(type) {
	case *packet.LevelChunk:
		if pk.Dimension != 0 {
			return nil
		}
		if i, ok := c.gallery.byChunk[world.ChunkPos(pk.Position)]; ok {
			return c.Conn.WritePacket(&packet.PrimitiveShapes{Shapes: []protocol.PrimitiveShape{c.gallery.label(i)}})
		}
	case *packet.SubChunk:
		if pk.Dimension != 0 {
			return nil
		}
		for _, entry := range pk.SubChunkEntries {
			if entry.Result != protocol.SubChunkResultSuccess || pk.Position[1]+int32(entry.Offset[1]) != (galleryGround+1)>>4 {
				continue
			}
			pos := world.ChunkPos{pk.Position[0] + int32(entry.Offset[0]), pk.Position[2] + int32(entry.Offset[2])}
			if i, ok := c.gallery.byChunk[pos]; ok {
				for _, actor := range c.gallery.actors(c.gallery.states[i]) {
					if err := c.Conn.WritePacket(actor); err != nil {
						return err
					}
				}
			}
		}
	}
	return nil
}

// label shows the exact identifier and properties above one cell's north edge.
func (g *blockGallery) label(i int) protocol.PrimitiveShape {
	s := g.states[i]
	return protocol.PrimitiveShape{
		NetworkID: uint64(galleryLabelFirstID + i), Type: protocol.Option(uint8(protocol.PrimitiveShapeText)),
		Location: protocol.Option(mgl32.Vec3{float32(s.Chunk[0])*16 + 8, galleryGround + 4, float32(s.Chunk[1])*16 + 1}),
		Scale:    protocol.Option(float32(1)), Colour: protocol.Option(color.RGBA{R: 255, G: 255, B: 255, A: 255}),
		DimensionID:    protocol.Option(int32(0)),
		ExtraShapeData: &protocol.TextShape{Text: s.Name + "\n" + strings.ReplaceAll(s.State, ",", ",\n"), DepthTest: true, ShowBackface: true, ShowBackfaceText: true, BackgroundColour: protocol.Option(color.RGBA{A: 180})},
	}
}

// actors supplies empty block-actor records even for states Dragonfly cannot simulate.
// Content variations, such as sign text or banner patterns, need a separate fixture.
func (g *blockGallery) actors(s galleryState) []*packet.BlockActorData {
	var actors []*packet.BlockActorData
	g.placements(s, func(p cube.Pos, id uint32) {
		name, _, _ := g.blocks.RuntimeIDToState(id)
		actor, ok := g.blocks.BlockActorID(name)
		if !ok {
			return
		}
		actors = append(actors, &packet.BlockActorData{Position: protocol.BlockPos{int32(p[0]), int32(p[1]), int32(p[2])}, NBTData: map[string]any{"id": actor}})
	})
	return actors
}
