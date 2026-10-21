package localserver

import (
	"io"
	"testing"

	"github.com/df-mc/dragonfly/server/entity"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl32"
	"github.com/go-gl/mathgl/mgl64"
	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

type flySpeedTestConn struct {
	session.Conn
	packets []packet.Packet
}

// ReadPacket returns a finite input stream without a network or background reader.
func (c *flySpeedTestConn) ReadPacket() (packet.Packet, error) {
	if len(c.packets) == 0 {
		return nil, io.EOF
	}
	pk := c.packets[0]
	c.packets = c.packets[1:]
	return pk, nil
}

func TestFlySpeedConnAppliesEveryHandledInputAndPreservesPackets(t *testing.T) {
	w := world.Config{Synchronous: true, Entities: entity.DefaultRegistry}.New()
	t.Cleanup(func() { _ = w.Close() })
	handle := world.EntitySpawnOpts{Position: mgl64.Vec3{0, 100, 0}}.New(player.Type, player.Config{Name: "tester", GameMode: world.GameModeCreative})
	if err := w.Do(func(tx *world.Tx) {
		p := tx.AddEntity(handle).(*player.Player)
		h := &localPlayerHandler{}
		p.Handle(h)
		h.flight.set(p, 500)
	}).Err(); err != nil {
		t.Fatal(err)
	}
	first := flightInput(mgl32.Vec2{0, 1}, 90)
	second := flightInput(mgl32.Vec2{1, 0}, 0, packet.InputFlagJumping)
	second.Tick = 2
	third := flightInput(mgl32.Vec2{}, 0)
	third.Tick = 3
	ordinary := &packet.CommandRequest{CommandLine: "/tp 0 100 0"}
	inner := &flySpeedTestConn{packets: []packet.Packet{first, second, third, ordinary}}
	conn := &flySpeedConn{
		Conn: inner,
		id:   handle.UUID(),
		players: func(id uuid.UUID) (*world.EntityHandle, bool) {
			return handle, id == handle.UUID()
		},
	}
	read := func(want packet.Packet) {
		t.Helper()
		if got, err := conn.ReadPacket(); err != nil || got != want {
			t.Fatalf("packet was changed: got %v, %v; want %v", got, err, want)
		}
	}
	check := func(want mgl64.Vec3) {
		t.Helper()
		if err := handle.Do(func(_ *world.Tx, e world.Entity) {
			if got := e.(*player.Player).Velocity(); got.Sub(want).Len() > 1e-9 {
				t.Fatalf("velocity = %v; want %v", got, want)
			}
		}).Err(); err != nil {
			t.Fatal(err)
		}
	}
	read(first)
	check(mgl64.Vec3{})
	// Dragonfly handles this transition after ReadPacket returns, before the next read.
	if err := handle.Do(func(_ *world.Tx, e world.Entity) { e.(*player.Player).StartFlying() }).Err(); err != nil {
		t.Fatal(err)
	}
	read(second)
	check(mgl64.Vec3{-25, 0, 0})
	read(third)
	check(mgl64.Vec3{25, 25, 0})
	read(ordinary)
	check(mgl64.Vec3{})
	if _, err := conn.ReadPacket(); err != io.EOF {
		t.Fatalf("read error = %v; want EOF", err)
	}
}

func TestFlySpeedConnDropsInputForDepartedPlayer(t *testing.T) {
	input := flightInput(mgl32.Vec2{0, 1}, 0)
	conn := &flySpeedConn{
		Conn:    &flySpeedTestConn{packets: []packet.Packet{input}},
		players: func(uuid.UUID) (*world.EntityHandle, bool) { return nil, false },
	}
	if got, err := conn.ReadPacket(); err != nil || got != input {
		t.Fatalf("input = %v, %v", got, err)
	}
	if _, err := conn.ReadPacket(); err != io.EOF {
		t.Fatalf("departed player changed read error: %v", err)
	}
	if conn.pending != nil {
		t.Fatal("departed player's input was retained")
	}
}
