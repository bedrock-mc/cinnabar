package localserver

import (
	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// flySpeedListener applies motion flight to local connections, preserving other packet wrappers.
func flySpeedListener(inner func(server.Config) (server.Listener, error), players func(uuid.UUID) (*world.EntityHandle, bool)) func(server.Config) (server.Listener, error) {
	return func(conf server.Config) (server.Listener, error) {
		listener, err := inner(conf)
		if err != nil {
			return nil, err
		}
		return &flySpeedPacketListener{Listener: listener, players: players}, nil
	}
}

type flySpeedPacketListener struct {
	server.Listener
	players func(uuid.UUID) (*world.EntityHandle, bool)
}

// Accept associates input with the connection's player identity.
func (l *flySpeedPacketListener) Accept() (session.Conn, error) {
	conn, err := l.Listener.Accept()
	if err != nil {
		return nil, err
	}
	id, err := uuid.Parse(conn.IdentityData().Identity)
	if err != nil {
		return conn, nil
	}
	return &flySpeedConn{Conn: conn, players: l.players, id: id, remote: conn.RemoteAddr().String()}, nil
}

// Disconnect unwraps motion flight before delegating to the underlying listener.
func (l *flySpeedPacketListener) Disconnect(conn session.Conn, reason string) error {
	if wrapped, ok := conn.(*flySpeedConn); ok {
		conn = wrapped.Conn
	}
	return l.Listener.Disconnect(conn, reason)
}

type flySpeedConn struct {
	session.Conn
	players func(uuid.UUID) (*world.EntityHandle, bool)
	id      uuid.UUID
	remote  string
	handle  *world.EntityHandle
	pending *packet.PlayerAuthInput
}

// ReadPacket applies the previous input before blocking for another packet.
// Dragonfly handles each returned packet synchronously before calling ReadPacket
// again, so flight toggles and position updates have already reached the world owner.
func (c *flySpeedConn) ReadPacket() (packet.Packet, error) {
	if input := c.pending; input != nil {
		c.pending = nil
		handle := c.handle
		if handle == nil {
			handle, _ = c.players(c.id)
		}
		if handle != nil {
			task := handle.Do(func(_ *world.Tx, e world.Entity) {
				p, ok := e.(*player.Player)
				if !ok {
					return
				}
				if c.handle == nil {
					// A reconnect may have the same UUID but belongs to a different connection.
					if addr := p.Addr(); addr != nil && addr.String() != c.remote {
						return
					}
					c.handle = handle
				}
				if h, ok := p.Handler().(*localPlayerHandler); ok {
					h.flight.tick(p, input)
				}
			})
			<-task.Done()
			if err := task.Err(); err != nil {
				return nil, err
			}
		}
	}
	pk, err := c.Conn.ReadPacket()
	if input, ok := pk.(*packet.PlayerAuthInput); ok && err == nil {
		c.pending = input
	}
	return pk, err
}

// WritePacketImmediate preserves immediate disconnect delivery through the wrapper.
func (c *flySpeedConn) WritePacketImmediate(packets ...packet.Packet) error {
	if immediate, ok := c.Conn.(interface {
		WritePacketImmediate(...packet.Packet) error
	}); ok {
		return immediate.WritePacketImmediate(packets...)
	}
	for _, pk := range packets {
		if err := c.Conn.WritePacket(pk); err != nil {
			return err
		}
	}
	return c.Conn.Flush()
}
