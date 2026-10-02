package proxy

import (
	"errors"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// upstreamRelayDisconnect records which connection produced a disconnect. A
// reverse-direction write can observe the upstream close before its reader does.
type upstreamRelayDisconnect struct {
	cause error
	value packet.Disconnect
}

func (e *upstreamRelayDisconnect) Error() string { return e.cause.Error() }
func (e *upstreamRelayDisconnect) Unwrap() error { return e.cause }

// upstreamRelayClose leaves queued upstream batches readable after its writer sees EOF.
type upstreamRelayClose struct{ error }

// Unwrap preserves the transport's close classification.
func (e *upstreamRelayClose) Unwrap() error { return e.error }

// attributeRelayError tags upstream failures while leaving successful and downstream results unchanged.
func attributeRelayError(err error, fromUpstream bool) error {
	if err == nil || !fromUpstream {
		return err
	}
	var disconnect *minecraft.DisconnectPacketError
	if errors.As(err, &disconnect) && disconnect != nil {
		return &upstreamRelayDisconnect{cause: err, value: *disconnect.Packet()}
	}
	if isOrdinaryClose(err) {
		return &upstreamRelayClose{error: err}
	}
	return err
}

type packetDisconnecter interface {
	DisconnectPacket(packet.Disconnect) error
}

// relayPreLoginDisconnect tells a downstream that has not spawned yet why its join failed: a
// server's own disconnect packet found anywhere in err, else vanilla's lang key for the failure.
func relayPreLoginDisconnect(downstream packetDisconnecter, err error) {
	var disconnect *minecraft.DisconnectPacketError
	if errors.As(err, &disconnect) && disconnect != nil {
		_ = callWithoutPanic(func() error { return downstream.DisconnectPacket(*disconnect.Packet()) })
		return
	}
	var cancelled *preparationCancellationError
	if err == nil || errors.As(err, &cancelled) {
		return
	}
	_ = callWithoutPanic(func() error { return downstream.DisconnectPacket(packet.Disconnect{Message: joinFailureKey(err)}) })
}

func joinFailureKey(err error) string {
	var realm *realmJoinError
	var admission *PackAdmissionError
	switch {
	case errors.Is(err, errResourcePackTransferTooLarge), errors.As(err, &admission):
		return "disconnectionScreen.resourcePack"
	case errors.As(err, &realm):
		return "disconnectionScreen.cantConnectToRealm"
	default:
		return "disconnectionScreen.cantConnect"
	}
}
