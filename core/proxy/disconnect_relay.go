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

func attributeRelayError(err error, fromUpstream bool) error {
	var disconnect *minecraft.DisconnectPacketError
	if fromUpstream && errors.As(err, &disconnect) && disconnect != nil {
		return &upstreamRelayDisconnect{cause: err, value: *disconnect.Packet()}
	}
	return err
}
