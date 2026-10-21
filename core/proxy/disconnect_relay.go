package proxy

import (
	"errors"

	"github.com/go-jose/go-jose/v4/jwt"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
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
	if streamnet.IsClosed(err) {
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
		_ = callSafely("delivering pre-login disconnect", func() error { return downstream.DisconnectPacket(*disconnect.Packet()) })
		return
	}
	var cancelled *preparationCancellationError
	if err == nil || errors.As(err, &cancelled) {
		return
	}
	_ = callSafely("delivering pre-login disconnect", func() error { return downstream.DisconnectPacket(packet.Disconnect{Message: joinFailureKey(err)}) })
}

// joinFailureKey names the lang key a failed join reads as. A sign-in token the local clock
// sees as not yet issued or valid means this device's clock is behind the identity service's.
func joinFailureKey(err error) string {
	var realm *realmJoinError
	var admission *PackAdmissionError
	switch {
	case errors.Is(err, errResourcePackSizeUnsupported), errors.As(err, &admission):
		return bridgecontract.DisconnectKeyResourcePack
	case errors.Is(err, jwt.ErrIssuedInTheFuture), errors.Is(err, jwt.ErrNotValidYet):
		return bridgecontract.DisconnectKeyDeviceClock
	case errors.As(err, &realm):
		return bridgecontract.DisconnectKeyCantConnectToRealm
	default:
		return bridgecontract.DisconnectKeyCantConnect
	}
}
