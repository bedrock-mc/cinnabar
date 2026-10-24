package main

import (
	"context"
	"strings"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl32"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

const totemFixtureActor = 9100400

// totemTestListener enables activation packets only in opted-in local worlds.
func totemTestListener(inner func(server.Config) (server.Listener, error)) func(server.Config) (server.Listener, error) {
	return func(conf server.Config) (server.Listener, error) {
		listener, err := inner(conf)
		if err != nil {
			return nil, err
		}
		return &totemListener{Listener: listener}, nil
	}
}

type totemListener struct{ server.Listener }

// Accept gives each connection its own player identity and fixture position.
func (l *totemListener) Accept() (session.Conn, error) {
	conn, err := l.Listener.Accept()
	if err != nil {
		return nil, err
	}
	return &totemTestConn{Conn: conn}, nil
}

// Disconnect restores the original connection before delegating.
func (l *totemListener) Disconnect(conn session.Conn, reason string) error {
	if wrapped, ok := conn.(*totemTestConn); ok {
		conn = wrapped.Conn
	}
	return l.Listener.Disconnect(conn, reason)
}

type totemTestConn struct {
	session.Conn
	playerID uint64
	position mgl32.Vec3
}

// StartGameContext retains the assigned player before the transport consumes initialisation.
func (c *totemTestConn) StartGameContext(ctx context.Context, data minecraft.GameData) error {
	c.playerID = data.EntityRuntimeID
	return c.Conn.StartGameContext(ctx, data)
}

// ReadPacket sends an equip, activation and consumption in a single ordered fixture burst.
func (c *totemTestConn) ReadPacket() (packet.Packet, error) {
	for {
		pk, err := c.Conn.ReadPacket()
		if err != nil {
			return pk, err
		}
		switch update := pk.(type) {
		case *packet.SetLocalPlayerAsInitialised:
			c.playerID = update.EntityRuntimeID
		case *packet.PlayerAuthInput:
			c.position = update.Position
		}
		request, ok := pk.(*packet.CommandRequest)
		if !ok || c.playerID == 0 {
			return pk, nil
		}
		fields := strings.Fields(strings.TrimPrefix(request.CommandLine, "/"))
		if len(fields) != 2 || fields[0] != "totemtest" {
			return pk, nil
		}
		fixtures := totemFixturePackets(fields[1], c.playerID, c.position)
		if len(fixtures) == 0 {
			return pk, nil
		}
		for _, fixture := range fixtures {
			if err := c.Conn.WritePacket(fixture); err != nil {
				return nil, err
			}
		}
	}
}

// totemFixturePackets preserves wire ordering and uses the server's negotiated item ID.
func totemFixturePackets(mode string, playerID uint64, position mgl32.Vec3) []packet.Packet {
	packets := []packet.Packet{}
	if mode == "clear" {
		return append(packets, &packet.RemoveActor{EntityUniqueID: totemFixtureActor})
	}
	if mode == "remote" {
		return append(packets,
			&packet.AddActor{EntityUniqueID: totemFixtureActor, EntityRuntimeID: totemFixtureActor,
				EntityType: "minecraft:zombie", Position: position.Add(mgl32.Vec3{0, -1.6, 4})},
			&packet.ActorEvent{EntityRuntimeID: totemFixtureActor, EventType: packet.ActorEventTalismanActivate},
		)
	}
	if mode != "offhand" && mode != "main" && mode != "empty" && mode != "idle" && mode != "staleecho" {
		return packets
	}
	runtimeID, metadata, ok := world.ItemRuntimeID(item.Totem{})
	if !ok {
		return packets
	}
	stack := protocol.ItemInstance{StackNetworkID: 1, Stack: protocol.ItemStack{
		ItemType: protocol.ItemType{NetworkID: runtimeID, MetadataValue: uint32(metadata)},
		Count:    1, NBTData: map[string]any{}, CanBePlacedOn: []string{}, CanBreak: []string{},
	}}
	window := uint32(protocol.WindowIDOffHand)
	if mode == "staleecho" {
		// The player's own offhand equipment echo with an older empty stack, while the offhand
		// still holds its item; a vanilla client ignores it for the local player.
		return append(packets, &packet.MobEquipment{EntityRuntimeID: playerID, WindowID: byte(window)})
	}
	if mode == "main" {
		packets = append(packets, &packet.InventorySlot{WindowID: window, Slot: 0})
		window = protocol.WindowIDInventory
		packets = append(packets, &packet.PlayerHotBar{
			WindowID: byte(window), SelectedHotBarSlot: 0, SelectHotBarSlot: true,
		})
	}
	if mode == "empty" {
		stack = protocol.ItemInstance{}
		packets = append(packets, &packet.InventorySlot{WindowID: protocol.WindowIDOffHand, Slot: 0})
		window = protocol.WindowIDInventory
	}
	packets = append(packets,
		&packet.InventorySlot{WindowID: window, Slot: 0, NewItem: stack},
		&packet.MobEquipment{EntityRuntimeID: playerID, NewItem: stack, WindowID: byte(window)},
	)
	if mode == "idle" {
		return packets
	}
	packets = append(packets,
		&packet.ActorEvent{EntityRuntimeID: playerID, EventType: packet.ActorEventTalismanActivate},
		&packet.LevelEvent{EventType: packet.LevelEventSoundTotemUsed, Position: position},
		&packet.InventorySlot{WindowID: window, Slot: 0},
		&packet.MobEquipment{EntityRuntimeID: playerID, WindowID: byte(window)},
	)
	return packets
}
