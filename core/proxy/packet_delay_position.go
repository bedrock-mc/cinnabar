package proxy

import (
	"math"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// ForwardedPosition is the movement packet's network anchor, not a server acknowledgement.
// The client converts this anchor to feet using its protocol-owned player offset.
type ForwardedPosition struct {
	X float32 `json:"x"`
	Y float32 `json:"y"`
	Z float32 `json:"z"`
}

func (delay *PacketDelay) clearPositionLocked() {
	delay.position = nil
	delay.positionEpoch++
}

func (delay *PacketDelay) positionToken() (uint64, bool) {
	if delay == nil {
		return 0, false
	}
	delay.mu.Lock()
	defer delay.mu.Unlock()
	return delay.positionEpoch, delay.showPosition && delay.delay != 0 && time.Now().Before(delay.expires)
}

// PositionSnapshot returns only an opted-in witness from the current unexpired lease.
func (delay *PacketDelay) PositionSnapshot() (uint64, *ForwardedPosition) {
	if delay == nil {
		return 0, nil
	}
	delay.mu.Lock()
	defer delay.mu.Unlock()
	if !delay.showPosition || delay.delay == 0 || !time.Now().Before(delay.expires) || delay.position == nil {
		return delay.session, nil
	}
	copy := *delay.position
	return delay.session, &copy
}

func (delay *PacketDelay) commitPosition(session, epoch uint64, position *ForwardedPosition) {
	if delay == nil || position == nil {
		return
	}
	delay.mu.Lock()
	defer delay.mu.Unlock()
	if delay.session == session && delay.positionEpoch == epoch && delay.showPosition && delay.delay != 0 && time.Now().Before(delay.expires) {
		copy := *position
		delay.position = &copy
	}
}

func isOwnMovement(id uint32) bool {
	return id == packet.IDPlayerAuthInput || id == packet.IDMovePlayer
}

func ownRuntimeID(source, destination packetSession) uint64 {
	for _, session := range []packetSession{source, destination} {
		if data, ok := session.(interface{ GameData() minecraft.GameData }); ok {
			return data.GameData().EntityRuntimeID
		}
	}
	return 0
}

func ownMovementPosition(raw minecraft.RawPacket, ownID uint64) *ForwardedPosition {
	var position *ForwardedPosition
	for _, decoded := range raw.Decoded {
		switch value := decoded.(type) {
		case *packet.PlayerAuthInput:
			position = &ForwardedPosition{value.Position[0], value.Position[1], value.Position[2]}
		case *packet.MovePlayer:
			if ownID != 0 && value.EntityRuntimeID == ownID && value.Mode != packet.MoveModeRotation {
				position = &ForwardedPosition{value.Position[0], value.Position[1], value.Position[2]}
			}
		}
	}
	if position != nil && (!finitePosition(position.X) || !finitePosition(position.Y) || !finitePosition(position.Z)) {
		return nil
	}
	return position
}

func finitePosition(value float32) bool {
	return !math.IsNaN(float64(value)) && !math.IsInf(float64(value), 0)
}
