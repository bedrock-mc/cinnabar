package proxy

import (
	"context"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// Direction is the way a relayed packet travels.
type Direction uint8

const (
	// FromClient is a packet the local client sent toward the server.
	FromClient Direction = iota
	// FromServer is a packet the server sent toward the local client.
	FromServer
)

// Verdict is what the relay does with an intercepted packet.
type Verdict uint8

const (
	// Forward relays the packet exactly as it was received.
	Forward Verdict = iota
	// ForwardModified re-encodes the packet, carrying any changes made to it in place.
	ForwardModified
	// Drop relays nothing in the packet's place.
	Drop
)

// Interceptor joins the packet relay of every session the core serves, as an in-process extension
// built into the core binary.
type Interceptor interface {
	// InterceptSession is called once per session after its upstream join reaches StartGame and its
	// resource packs are selected, before the client receives the handoff. An error refuses the
	// session; a nil SessionInterceptor relays the session untouched.
	InterceptSession(ctx context.Context, session *InterceptedSession) (SessionInterceptor, error)
}

// SessionInterceptor sees one session's packets as the relay forwards them. Calls for one direction
// come from that direction's relay goroutine in order; the two directions run concurrently. An error
// or panic ends the session.
type SessionInterceptor interface {
	// Intercepts reports whether packets with id travelling in direction are decoded and passed to
	// HandlePacket; all others are relayed as received. It must be cheap and must not change during
	// the session. A selected server packet that fails to decode is dropped.
	Intercepts(direction Direction, id uint32) bool
	// BeginBatch is called before each network batch travelling in direction is relayed.
	BeginBatch(direction Direction)
	// HandlePacket receives each intercepted packet in its turn. Packets written toward the same
	// destination during the call travel ahead of it, in the same batch.
	HandlePacket(direction Direction, pk packet.Packet) (Verdict, error)
	// EndBatch is called after each batch travelling in direction is relayed and before it is
	// flushed, so packets written toward its destination during the call end the same batch.
	EndBatch(direction Direction) error
	// Close is called once when the session ends, with the cause; nil is an ordinary close.
	Close(cause error)
}

// ResourcePackProvider is implemented by a SessionInterceptor that adds resource packs to its
// session. The core calls it once, after InterceptSession returns.
type ResourcePackProvider interface {
	// ResourcePacks returns packs applied above the server's packs and below them, each highest
	// priority first. A pack the server's stack already selected keeps its server position.
	ResourcePacks() (above, below []*resource.Pack)
}

// InterceptedSession is one relayed session as an Interceptor sees it. Its methods are safe for
// concurrent use once InterceptSession has returned.
type InterceptedSession struct {
	client     packetSession
	server     upstreamSession
	gameData   minecraft.GameData
	identity   login.IdentityData
	clientData login.ClientData
	latency    func() time.Duration
}

// GameData returns the game data the upstream server's StartGame established.
func (session *InterceptedSession) GameData() minecraft.GameData { return session.gameData }

// IdentityData returns the canonical player identity of the upstream login.
func (session *InterceptedSession) IdentityData() login.IdentityData { return session.identity }

// ClientData returns the client data the core claimed in the upstream login.
func (session *InterceptedSession) ClientData() login.ClientData { return session.clientData }

// Latency returns the upstream connection's current half round-trip estimate, or zero when its
// transport has none.
func (session *InterceptedSession) Latency() time.Duration { return sampleLatency(session.latency) }

// WriteToClient queues pk in the batch forming toward the client.
func (session *InterceptedSession) WriteToClient(pk packet.Packet) error {
	return session.client.WritePacket(pk)
}

// WriteToServer queues pk in the batch forming toward the server.
func (session *InterceptedSession) WriteToServer(pk packet.Packet) error {
	return session.server.WritePacket(pk)
}

// FlushClient sends the batch forming toward the client. The relay flushes after each batch it
// forwards, so only packets written outside HandlePacket and EndBatch need an explicit flush to
// leave sooner than the relay's idle flush.
func (session *InterceptedSession) FlushClient() error { return session.client.Flush() }

// FlushServer sends the batch forming toward the server, as FlushClient does toward the client.
func (session *InterceptedSession) FlushServer() error { return session.server.Flush() }

// interceptSession asks interceptor to join a prepared session, containing a panic as an error.
func interceptSession(ctx context.Context, interceptor Interceptor, session *InterceptedSession) (intercept SessionInterceptor, err error) {
	defer func() {
		if recovered := recover(); recovered != nil {
			intercept, err = nil, panicTypeError("starting session interceptor", recovered)
		}
	}()
	return interceptor.InterceptSession(ctx, session)
}

// interceptedResourcePacks returns the packs intercept adds, containing a panic as an error.
func interceptedResourcePacks(intercept SessionInterceptor) (above, below []*resource.Pack, err error) {
	provider, ok := intercept.(ResourcePackProvider)
	if !ok {
		return nil, nil, nil
	}
	defer func() {
		if recovered := recover(); recovered != nil {
			above, below, err = nil, nil, panicTypeError("listing interceptor resource packs", recovered)
		}
	}()
	above, below = provider.ResourcePacks()
	return above, below, nil
}

// closeIntercept ends intercept once its session has ended; a panic is contained.
func closeIntercept(intercept SessionInterceptor, cause error) {
	if intercept == nil {
		return
	}
	defer func() { _ = recover() }()
	intercept.Close(cause)
}

// interceptDecode widens decode with the packets intercept selects in direction.
func interceptDecode(intercept SessionInterceptor, direction Direction, decode func(uint32) bool) func(uint32) bool {
	if intercept == nil {
		return decode
	}
	return func(id uint32) bool {
		return intercept.Intercepts(direction, id) || decode != nil && decode(id)
	}
}
