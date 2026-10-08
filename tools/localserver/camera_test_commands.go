package main

import (
	"strings"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/go-gl/mathgl/mgl32"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// cameraTestListener enables packet fixtures only for explicitly opted-in local servers.
func cameraTestListener(inner func(server.Config) (server.Listener, error)) func(server.Config) (server.Listener, error) {
	return func(conf server.Config) (server.Listener, error) {
		listener, err := inner(conf)
		if err != nil {
			return nil, err
		}
		return &cameraListener{Listener: listener}, nil
	}
}

type cameraListener struct{ server.Listener }

// Accept wraps the connection so fixture commands never reach normal command handling.
func (l *cameraListener) Accept() (session.Conn, error) {
	c, err := l.Listener.Accept()
	if err != nil {
		return nil, err
	}
	return &cameraTestConn{Conn: c}, nil
}

// Disconnect gives the underlying listener its original connection.
func (l *cameraListener) Disconnect(c session.Conn, reason string) error {
	if wrapped, ok := c.(*cameraTestConn); ok {
		c = wrapped.Conn
	}
	return l.Listener.Disconnect(c, reason)
}

type cameraTestConn struct {
	session.Conn
	position mgl32.Vec3
}

// ReadPacket emits deterministic fixture traffic around the last reported player position.
func (c *cameraTestConn) ReadPacket() (packet.Packet, error) {
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
		if len(fields) != 2 || fields[0] != "cameratest" {
			return pk, nil
		}
		fixtures := cameraFixturePackets(fields[1], c.position)
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
