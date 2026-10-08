package main

import (
	"context"
	"errors"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"testing"
)

type commandFlagConn struct {
	session.Conn
	started    bool
	startError error
	packets    []packet.Packet
}

// StartGameContext records handshake ordering without a network connection.
func (c *commandFlagConn) StartGameContext(context.Context, minecraft.GameData) error {
	c.started = true
	return c.startError
}

// WritePacket records the permission update after the handshake.
func (c *commandFlagConn) WritePacket(pk packet.Packet) error {
	c.packets = append(c.packets, pk)
	return nil
}

func TestManagedWorldDisablesTransportCommandDefault(t *testing.T) {
	conn := &commandFlagConn{}
	wrapped := commandsDisabledConn{Conn: conn}
	if err := wrapped.StartGameContext(context.Background(), minecraft.GameData{}); err != nil {
		t.Fatal(err)
	}
	if !conn.started || len(conn.packets) != 1 {
		t.Fatal("missing command permission after handshake")
	}
	flag, ok := conn.packets[0].(*packet.SetCommandsEnabled)
	if !ok || flag.Enabled {
		t.Fatal("managed world still advertises enabled cheats")
	}
	conn.startError = errors.New("handshake failed")
	conn.packets = nil
	if err := wrapped.StartGameContext(context.Background(), minecraft.GameData{}); err == nil || len(conn.packets) != 0 {
		t.Fatal("failed handshake published a command update")
	}
}
