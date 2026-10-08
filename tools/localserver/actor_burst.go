package main

import (
	"fmt"
	"strconv"
	"strings"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/go-gl/mathgl/mgl32"
	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

const actorBurstLimit = 256
const actorBurstFirstID = 9100000

// actorBurstListener enables generated player bursts only in opted-in local worlds.
func actorBurstListener(inner func(server.Config) (server.Listener, error), artwork int) func(server.Config) (server.Listener, error) {
	return func(conf server.Config) (server.Listener, error) {
		listener, err := inner(conf)
		if err != nil {
			return nil, err
		}
		return &burstListener{Listener: listener, artwork: artwork}, nil
	}
}

type burstListener struct {
	server.Listener
	artwork int
}

// Accept gives each connection its own synthetic actors and command state.
func (l *burstListener) Accept() (session.Conn, error) {
	conn, err := l.Listener.Accept()
	if err != nil {
		return nil, err
	}
	return &actorBurstConn{Conn: conn, artwork: l.artwork}, nil
}

// Disconnect unwraps this fixture before delegating to the original listener.
func (l *burstListener) Disconnect(conn session.Conn, reason string) error {
	if wrapped, ok := conn.(*actorBurstConn); ok {
		conn = wrapped.Conn
	}
	return l.Listener.Disconnect(conn, reason)
}

type actorBurstConn struct {
	session.Conn
	position mgl32.Vec3
	count    int
	mode     string
	artwork  int
}

// ReadPacket replaces the previous crowd atomically in wire order around the latest player position.
func (c *actorBurstConn) ReadPacket() (packet.Packet, error) {
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
		count, mode, ok := actorBurstCommand(request.CommandLine)
		if !ok || (mode == "artwork" && count > c.artwork) {
			return pk, nil
		}
		packets := actorBurstClear(c.count, c.mode)
		if count > 0 {
			packets = append(packets, actorBurstPackets(count, mode, c.position)...)
		}
		for _, fixture := range packets {
			if err := c.Conn.WritePacket(fixture); err != nil {
				return nil, err
			}
		}
		c.count, c.mode = count, mode
	}
}

// actorBurstCommand bounds fixture size and leaves malformed or unrelated commands untouched.
func actorBurstCommand(command string) (int, string, bool) {
	fields := strings.Fields(strings.TrimPrefix(command, "/"))
	if len(fields) == 2 && fields[0] == "actorburst" && fields[1] == "clear" {
		return 0, "", true
	}
	if len(fields) != 3 || fields[0] != "actorburst" {
		return 0, "", false
	}
	count, err := strconv.Atoi(fields[1])
	if err != nil || count < 1 || count > actorBurstLimit {
		return 0, "", false
	}
	switch fields[2] {
	case "shared", "skins", "geometry", "complex", "artwork":
		return count, fields[2], true
	}
	return 0, "", false
}

// actorBurstUUID gives the list and actor packets the same stable, synthetic identity.
func actorBurstUUID(index int) uuid.UUID {
	return uuid.NewSHA1(uuid.Nil, []byte(fmt.Sprintf("cinnabar-actor-burst-%d", index)))
}

// actorBurstPackets announces every generated appearance before spawning its actor.
func actorBurstPackets(count int, mode string, origin mgl32.Vec3) []packet.Packet {
	if mode == "artwork" {
		return actorBurstArtworkPackets(count, origin)
	}
	list := &packet.PlayerList{Entries: make([]protocol.PlayerListEntry, count)}
	packets := make([]packet.Packet, 1, count+1)
	packets[0] = list
	for index := range count {
		id, name := int64(actorBurstFirstID+index), fmt.Sprintf("Fixture %03d", index+1)
		variant, model := 0, 0
		if mode != "shared" {
			variant = index
		}
		if mode == "geometry" || mode == "complex" {
			model = index
		}
		skin := actorBurstSkin(variant, model)
		if mode == "complex" {
			skin.SkinGeometry = actorBurstComplexGeometry(model)
		}
		list.Entries[index] = protocol.PlayerListEntry{
			ActionType: protocol.PlayerListActionAdd, UUID: actorBurstUUID(index),
			EntityUniqueID: id, Username: name, Skin: skin,
		}
		packets = append(packets, &packet.AddPlayer{
			UUID: actorBurstUUID(index), Username: name, EntityRuntimeID: uint64(id),
			Position:       actorBurstPosition(index, origin),
			EntityMetadata: actorBurstMetadata(),
			AbilityData:    protocol.AbilityData{EntityUniqueID: id},
		})
	}
	return packets
}

// actorBurstClear removes only the previous fixture crowd and its player-list entries.
func actorBurstClear(count int, mode string) []packet.Packet {
	if count == 0 {
		return nil
	}
	packets := make([]packet.Packet, 0, count+1)
	list := &packet.PlayerList{Entries: make([]protocol.PlayerListEntry, count)}
	for index := range count {
		packets = append(packets, &packet.RemoveActor{EntityUniqueID: int64(actorBurstFirstID + index)})
		list.Entries[index] = protocol.PlayerListEntry{ActionType: protocol.PlayerListActionRemove, UUID: actorBurstUUID(index)}
	}
	if mode == "artwork" {
		return packets
	}
	return append(packets, list)
}

// actorBurstMetadata gives player and custom-entity fixtures the same body bounds.
func actorBurstMetadata() protocol.EntityMetadata {
	return protocol.EntityMetadata{protocol.EntityDataKeyWidth: float32(0.6), protocol.EntityDataKeyHeight: float32(1.8)}
}
