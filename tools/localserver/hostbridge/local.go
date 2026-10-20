package hostbridge

import (
	"sync"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// WrapLocal shares actual capacity without changing the local listener's transport or identity policy.
func (l *Listener) WrapLocal(factory func(server.Config) (server.Listener, error)) func(server.Config) (server.Listener, error) {
	return func(conf server.Config) (server.Listener, error) {
		inner, err := factory(conf)
		if err != nil {
			return nil, err
		}
		return &localListener{Listener: inner, owner: l}, nil
	}
}

type localListener struct {
	server.Listener
	owner *Listener
}

// Accept keeps a local reconnect inside the same actual process cap through Conn.Close.
func (l *localListener) Accept() (session.Conn, error) {
	for {
		conn, err := l.Listener.Accept()
		if err != nil {
			return nil, err
		}
		release, ok := l.owner.reserve()
		if !ok {
			_ = l.Listener.Disconnect(conn, "Server is full.")
			_ = conn.Close()
			continue
		}
		return &localConn{Conn: conn, release: release}, nil
	}
}

// Disconnect unwraps only the original owner before the underlying listener's concrete assertion.
func (l *localListener) Disconnect(conn session.Conn, reason string) error {
	if c, ok := conn.(*localConn); ok {
		err := l.Listener.Disconnect(c.Conn, reason)
		_ = c.Close()
		return err
	}
	return errProtocol
}

type localConn struct {
	session.Conn
	release func()
	once    sync.Once
}

// Close releases capacity once, after closing its actual accepted local connection.
func (c *localConn) Close() error {
	err := c.Conn.Close()
	c.once.Do(c.release)
	return err
}

// WritePacketImmediate keeps the actual local transport's original immediate-disconnect behavior.
func (c *localConn) WritePacketImmediate(packets ...packet.Packet) error {
	if immediate, ok := c.Conn.(interface{ WritePacketImmediate(...packet.Packet) error }); ok {
		return immediate.WritePacketImmediate(packets...)
	}
	for _, pk := range packets {
		if err := c.Conn.WritePacket(pk); err != nil {
			return err
		}
	}
	return c.Conn.Flush()
}

// disconnect uses actual generated terminal output and the same finite ordered flush.
func disconnect(c *Conn, reason string) error {
	err := c.WritePacketImmediate(&packet.Disconnect{Message: reason})
	_ = c.Close()
	return err
}
