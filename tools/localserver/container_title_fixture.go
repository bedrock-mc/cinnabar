package main

import (
	"strings"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/go-gl/mathgl/mgl32"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

const (
	containerTitleActor  = 9100500
	containerTitleWindow = 90
	// containerTitleRaw and containerTitleFiltered are the fixture's name tag and filtered name.
	containerTitleRaw      = "Raw title"
	containerTitleFiltered = "Filtered title"
)

// containerTitleListener enables /containertitle only in opted-in local worlds.
func containerTitleListener(inner func(server.Config) (server.Listener, error)) func(server.Config) (server.Listener, error) {
	return func(conf server.Config) (server.Listener, error) {
		listener, err := inner(conf)
		if err != nil {
			return nil, err
		}
		return &containerTitleServerListener{Listener: listener}, nil
	}
}

type containerTitleServerListener struct{ server.Listener }

// Accept wraps the connection so fixture commands never reach normal command handling.
func (l *containerTitleServerListener) Accept() (session.Conn, error) {
	c, err := l.Listener.Accept()
	if err != nil {
		return nil, err
	}
	return &containerTitleConn{Conn: c}, nil
}

// Disconnect gives the underlying listener its original connection.
func (l *containerTitleServerListener) Disconnect(c session.Conn, reason string) error {
	if wrapped, ok := c.(*containerTitleConn); ok {
		c = wrapped.Conn
	}
	return l.Listener.Disconnect(c, reason)
}

type containerTitleConn struct {
	session.Conn
	position mgl32.Vec3
}

// ReadPacket answers /containertitle with an actor-backed chest menu near the player.
func (c *containerTitleConn) ReadPacket() (packet.Packet, error) {
	for {
		pk, err := c.Conn.ReadPacket()
		if err != nil {
			return pk, err
		}
		if movement, ok := pk.(*packet.PlayerAuthInput); ok {
			c.position = movement.Position
		}
		request, ok := pk.(*packet.CommandRequest)
		if !ok {
			return pk, nil
		}
		fields := strings.Fields(strings.TrimPrefix(request.CommandLine, "/"))
		if len(fields) != 2 || fields[0] != "containertitle" {
			return pk, nil
		}
		fixtures := containerTitlePackets(fields[1], c.position)
		if fixtures == nil {
			return pk, nil
		}
		for _, fixture := range fixtures {
			if err := c.Conn.WritePacket(fixture); err != nil {
				return nil, err
			}
		}
	}
}

// containerTitlePackets spawns a named chest minecart and opens its 27-slot menu. Mode
// "filtered" also syncs a filtered name, "raw" syncs only the name tag; other modes return nil.
func containerTitlePackets(mode string, position mgl32.Vec3) []packet.Packet {
	metadata := protocol.EntityMetadata{protocol.EntityDataKeyName: containerTitleRaw}
	switch mode {
	case "filtered":
		metadata[protocol.EntityDataKeyFilteredName] = containerTitleFiltered
	case "raw":
	default:
		return nil
	}
	return []packet.Packet{
		&packet.RemoveActor{EntityUniqueID: containerTitleActor},
		&packet.AddActor{
			EntityUniqueID:  containerTitleActor,
			EntityRuntimeID: containerTitleActor,
			EntityType:      "minecraft:chest_minecart",
			Position:        position.Add(mgl32.Vec3{0, -1.6, 3}),
			EntityMetadata:  metadata,
		},
		&packet.ContainerOpen{
			WindowID:                containerTitleWindow,
			ContainerType:           protocol.ContainerTypeContainer,
			ContainerEntityUniqueID: containerTitleActor,
		},
		&packet.InventoryContent{
			WindowID: containerTitleWindow,
			Content:  make([]protocol.ItemInstance, 27),
		},
	}
}
