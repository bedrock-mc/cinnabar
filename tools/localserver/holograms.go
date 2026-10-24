package main

import (
	"fmt"
	"strings"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/go-gl/mathgl/mgl32"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

const (
	hologramCount       = 150
	hologramFirstID     = 9200000
	hologramMaxLines    = 10
	hologramColourCount = 16
	hologramColumns     = 15
	hologramSpacing     = 2.8
)

// hologramListener enables the floating-text workload only for an opted-in local world.
func hologramListener(inner func(server.Config) (server.Listener, error)) func(server.Config) (server.Listener, error) {
	return func(conf server.Config) (server.Listener, error) {
		listener, err := inner(conf)
		if err != nil {
			return nil, err
		}
		return &hologramServerListener{Listener: listener}, nil
	}
}

type hologramServerListener struct{ server.Listener }

// Accept gives each client its own deterministic labels and animation tick.
func (l *hologramServerListener) Accept() (session.Conn, error) {
	conn, err := l.Listener.Accept()
	if err != nil {
		return nil, err
	}
	return &hologramConn{Conn: conn, initial: true}, nil
}

// Disconnect unwraps the fixture connection for the owning listener.
func (l *hologramServerListener) Disconnect(conn session.Conn, reason string) error {
	if wrapped, ok := conn.(*hologramConn); ok {
		conn = wrapped.Conn
	}
	return l.Listener.Disconnect(conn, reason)
}

type hologramConn struct {
	session.Conn
	position                 mgl32.Vec3
	tick                     uint64
	initial, spawned, frozen bool
}

// ReadPacket advances text on input ticks and handles repeatable fixture resets in wire order.
func (c *hologramConn) ReadPacket() (packet.Packet, error) {
	for {
		pk, err := c.Conn.ReadPacket()
		if err != nil {
			return pk, err
		}
		var fixtures []packet.Packet
		if input, ok := pk.(*packet.PlayerAuthInput); ok {
			c.position = input.Position
			if c.initial {
				c.initial, c.spawned = false, true
				fixtures = hologramSpawn(c.position)
			} else if c.spawned && !c.frozen {
				c.tick++
				fixtures = hologramUpdates(c.tick)
			}
		}
		consumed := false
		if request, ok := pk.(*packet.CommandRequest); ok {
			command := strings.TrimSpace(strings.TrimPrefix(request.CommandLine, "/"))
			if command == "holograms" || command == "holograms freeze" || command == "holograms clear" {
				consumed = true
				if c.spawned {
					for index := range hologramCount {
						fixtures = append(fixtures, &packet.RemoveActor{EntityUniqueID: int64(hologramFirstID + index)})
					}
				}
				c.initial, c.spawned, c.frozen, c.tick = false, command != "holograms clear", command == "holograms freeze", 0
				if c.spawned {
					fixtures = append(fixtures, hologramSpawn(c.position)...)
				}
			}
		}
		for _, fixture := range fixtures {
			if err := c.Conn.WritePacket(fixture); err != nil {
				return nil, err
			}
		}
		if !consumed {
			return pk, nil
		}
	}
}

// hologramText builds formatted lines, changing text for every second marker.
func hologramText(index int, tick uint64) string {
	var text strings.Builder
	lines := 1 + index%hologramMaxLines
	for line := 0; line < lines; line++ {
		if line != 0 {
			text.WriteByte('\n')
		}
		colour := uint64(index+line) % hologramColourCount
		if index%2 != 0 && index%3 == 0 {
			colour = (colour + tick) % hologramColourCount
		}
		fmt.Fprintf(&text, "§%x", colour)
		if line == 0 && index%7 == 0 {
			text.WriteString("§l")
		}
		fmt.Fprintf(&text, "Board %03d · %02d", index+1, line+1)
		if index%11 == 0 && line == 0 {
			text.WriteString(" §kABCD§r")
		}
		if index%2 != 0 && line+1 == lines {
			fmt.Fprintf(&text, " %06d", tick)
		}
	}
	return text.String()
}

// hologramSpawn places zero-size marker actors in a fixed grid facing an origin looking south.
func hologramSpawn(origin mgl32.Vec3) []packet.Packet {
	packets := make([]packet.Packet, 0, hologramCount)
	for index := range hologramCount {
		metadata := protocol.EntityMetadata{
			protocol.EntityDataKeyFlags:             int64(0),
			protocol.EntityDataKeyName:              hologramText(index, 0),
			protocol.EntityDataKeyAlwaysShowNameTag: uint8(1),
			protocol.EntityDataKeyScale:             float32(0),
			protocol.EntityDataKeyWidth:             float32(0),
			protocol.EntityDataKeyHeight:            float32(0),
		}
		metadata.SetFlag(protocol.EntityDataKeyFlags, protocol.EntityDataFlagAlwaysShowName)
		metadata.SetFlag(protocol.EntityDataKeyFlags, protocol.EntityDataFlagShowName)
		packets = append(packets, &packet.AddActor{
			EntityUniqueID: int64(hologramFirstID + index), EntityRuntimeID: uint64(hologramFirstID + index),
			EntityType: "minecraft:armor_stand",
			Position: origin.Add(mgl32.Vec3{
				float32(index%hologramColumns-hologramColumns/2) * hologramSpacing,
				float32(index/hologramColumns%3) * 2,
				18 + float32(index/hologramColumns)*hologramSpacing,
			}),
			EntityMetadata: metadata,
		})
	}
	return packets
}

// hologramUpdates rewrites exactly half the actors, preserving every other metadata field.
func hologramUpdates(tick uint64) []packet.Packet {
	packets := make([]packet.Packet, 0, hologramCount/2)
	for index := 1; index < hologramCount; index += 2 {
		packets = append(packets, &packet.SetActorData{
			EntityRuntimeID: uint64(hologramFirstID + index),
			EntityMetadata:  protocol.EntityMetadata{protocol.EntityDataKeyName: hologramText(index, tick)},
		})
	}
	return packets
}
