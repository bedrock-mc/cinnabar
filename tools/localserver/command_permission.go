package main

import (
	"context"
	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// commandsDisabledListener publishes the managed world's command permission after login.
func commandsDisabledListener(inner func(server.Config) (server.Listener, error)) func(server.Config) (server.Listener, error) {
	return func(conf server.Config) (server.Listener, error) {
		listener, err := inner(conf)
		if err != nil {
			return nil, err
		}
		return &commandsDisabledServer{Listener: listener}, nil
	}
}

type commandsDisabledServer struct{ server.Listener }

// Accept retains connection behavior while replacing the command flag sent to the client.
func (l *commandsDisabledServer) Accept() (session.Conn, error) {
	conn, err := l.Listener.Accept()
	if err != nil {
		return nil, err
	}
	return &commandsDisabledConn{Conn: conn}, nil
}

// Disconnect restores the connection identity expected by the wrapped listener.
func (l *commandsDisabledServer) Disconnect(conn session.Conn, reason string) error {
	if wrapped, ok := conn.(*commandsDisabledConn); ok {
		conn = wrapped.Conn
	}
	return l.Listener.Disconnect(conn, reason)
}

type commandsDisabledConn struct{ session.Conn }

// StartGameContext overrides the transport's permissive command default before play.
func (c *commandsDisabledConn) StartGameContext(ctx context.Context, data minecraft.GameData) error {
	if err := c.Conn.StartGameContext(ctx, data); err != nil {
		return err
	}
	return c.Conn.WritePacket(&packet.SetCommandsEnabled{Enabled: false})
}
