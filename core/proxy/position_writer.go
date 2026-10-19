package proxy

import (
	"sync"

	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// positionWriter observes forwarded and injected movements at their shared upstream writer.
type positionWriter struct {
	packetSession
	mu       sync.Mutex
	delay    *PacketDelay
	session  uint64
	ownID    uint64
	armed    bool
	epoch    uint64
	position *ForwardedPosition
}

// positionWrites exposes the shared writer through its upstream adapter.
func (writer *positionWriter) positionWrites() *positionWriter { return writer }

// trackPositionWrites binds the shared writer before relay callbacks can inject packets.
func trackPositionWrites(destination packetSession, delay *PacketDelay, session, ownID uint64) packetSession {
	if delay == nil {
		return destination
	}
	var writer *positionWriter
	if observed, ok := destination.(interface{ positionWrites() *positionWriter }); ok {
		writer = observed.positionWrites()
	} else {
		writer = &positionWriter{packetSession: destination}
	}
	writer.mu.Lock()
	defer writer.mu.Unlock()
	if !writer.armed || writer.delay != delay || writer.session != session {
		writer.delay, writer.session, writer.armed = delay, session, true
		writer.position = nil
	}
	if ownID != 0 {
		writer.ownID = ownID
	}
	return writer
}

// observe records a movement only under the lease captured before its successful write.
func (writer *positionWriter) observe(position *ForwardedPosition, epoch uint64, enabled bool) {
	if writer.armed && enabled && position != nil {
		writer.epoch, writer.position = epoch, position
	}
}

// receivedPosition copies the received anchor before an interceptor can mutate decoded values.
func (writer *positionWriter) receivedPosition(values []packet.Packet) *ForwardedPosition {
	if len(values) == 0 {
		return nil
	}
	writer.mu.Lock()
	defer writer.mu.Unlock()
	return ownMovementPosition(values, writer.ownID)
}

// WritePacket observes typed writes, including injections made by either relay direction.
func (writer *positionWriter) WritePacket(value packet.Packet) error {
	writer.mu.Lock()
	defer writer.mu.Unlock()
	epoch, enabled := writer.delay.positionToken()
	if err := writer.packetSession.WritePacket(value); err != nil {
		return err
	}
	writer.observe(ownMovementPosition([]packet.Packet{value}, writer.ownID), epoch, enabled)
	return nil
}

// writeReceived observes decoded movements while preserving their original encoded bytes.
func (writer *positionWriter) writeReceived(data []byte, position *ForwardedPosition) error {
	writer.mu.Lock()
	defer writer.mu.Unlock()
	epoch, enabled := writer.delay.positionToken()
	if err := writer.packetSession.WritePacketRaw(data); err != nil {
		return err
	}
	writer.observe(position, epoch, enabled)
	return nil
}

// WritePacketRaw serializes raw writes; the relay supplies decoded observations through writeReceived.
func (writer *positionWriter) WritePacketRaw(data []byte) error {
	return writer.writeReceived(data, nil)
}

// Flush commits only movements that left the upstream buffer successfully.
func (writer *positionWriter) Flush() error {
	writer.mu.Lock()
	defer writer.mu.Unlock()
	position := writer.position
	writer.position = nil
	if err := writer.packetSession.Flush(); err != nil {
		return err
	}
	writer.delay.commitPosition(writer.session, writer.epoch, position)
	return nil
}

// WritePacketImmediate observes a successful immediate write and its flush atomically.
func (writer *positionWriter) WritePacketImmediate(values ...packet.Packet) error {
	writer.mu.Lock()
	defer writer.mu.Unlock()
	defer func() { writer.position = nil }()
	epoch, enabled := writer.delay.positionToken()
	if err := writer.packetSession.WritePacketImmediate(values...); err != nil {
		return err
	}
	writer.observe(ownMovementPosition(values, writer.ownID), epoch, enabled)
	writer.delay.commitPosition(writer.session, writer.epoch, writer.position)
	return nil
}

// positionUpstream preserves upstream metadata while all writes use one position observer.
type positionUpstream struct{ *positionWriter }

// observeUpstreamPositions shares the upstream writer with the session interceptor before relay.
func observeUpstreamPositions(upstream upstreamSession, delay *PacketDelay, ownID uint64) upstreamSession {
	if delay == nil {
		return upstream
	}
	return &positionUpstream{&positionWriter{packetSession: upstream, delay: delay, ownID: ownID}}
}

// IdentityData preserves the upstream account identity.
func (session *positionUpstream) IdentityData() login.IdentityData {
	return session.packetSession.(upstreamSession).IdentityData()
}

// ResourcePacks preserves the upstream pack selection.
func (session *positionUpstream) ResourcePacks() []*resource.Pack {
	return session.packetSession.(upstreamSession).ResourcePacks()
}

// TexturePacksRequired preserves the server's pack requirement.
func (session *positionUpstream) TexturePacksRequired() bool {
	return session.packetSession.(upstreamSession).TexturePacksRequired()
}
