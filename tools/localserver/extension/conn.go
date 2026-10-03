package extension

import (
	"errors"
	"fmt"
	"slices"
	"sync"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/experience"
)

// Listener wraps a Dragonfly listener function, typically the default RakNet one of
// server.UserConfig.Config, so that every connection it accepts carries the server half.
func (s *Server) Listener(inner func(server.Config) (server.Listener, error)) func(server.Config) (server.Listener, error) {
	return func(conf server.Config) (server.Listener, error) {
		l, err := inner(conf)
		if err != nil {
			return nil, err
		}
		return &listener{Listener: l, s: s}, nil
	}
}

// listener hands Dragonfly wrapped connections and gives its own listener back the connections
// it accepted: the default RakNet listener's Disconnect requires its *minecraft.Conn.
type listener struct {
	server.Listener
	s *Server
}

func (l *listener) Accept() (session.Conn, error) {
	c, err := l.Listener.Accept()
	if err != nil {
		return nil, err
	}
	player, err := uuid.Parse(c.IdentityData().Identity)
	if err != nil {
		// Dragonfly refuses such a connection itself; there is no player to offer anything to.
		return c, nil
	}
	return &Conn{Conn: c, s: l.s, player: player}, nil
}

func (l *listener) Disconnect(c session.Conn, reason string) error {
	if wrapped, ok := c.(*Conn); ok {
		wrapped.end("disconnected")
		c = wrapped.Conn
	}
	return l.Listener.Disconnect(c, reason)
}

// phase is where a connection is in the handshake.
type phase uint8

const (
	// awaitingHello is every connection's start: a vanilla client never leaves it.
	awaitingHello phase = iota
	// awaitingReady follows the Accept, while the client downloads and starts its bundles.
	awaitingReady
	// active carries typed messages both ways.
	active
	// fallenBack is final: the client part gets nothing more and its messages are dropped.
	fallenBack
)

// Conn is a player's connection with the server half on it. Reading intercepts the carrier's
// script messages and passes every other packet through untouched; writing passes everything
// through and watches for a dimension change, which resets the client's world epoch.
//
// Dragonfly's default listener does not expose packet headers, so the sub-client route of a
// carrier message is the one its body names, which must be 0; Dragonfly admits no sub-client
// login, so every packet on a connection is its primary client's.
type Conn struct {
	session.Conn
	s      *Server
	player uuid.UUID

	mu    sync.Mutex
	phase phase
	// hello and session are the Hello and the session that the Accept bound.
	hello   Hello
	session string
	expires uint64
	// grant, epoch, ingress, egress and sequence are Ready's session, set when active.
	grant    *Grant
	epoch    uint64
	ingress  *Ingress
	egress   RateLimit
	sequence uint64
}

// ReadPacket returns the next packet that is not on the carrier, handling carrier messages on the
// way.
func (c *Conn) ReadPacket() (packet.Packet, error) {
	for {
		pk, err := c.Conn.ReadPacket()
		if err != nil {
			return pk, err
		}
		msg, ok := pk.(*packet.ScriptMessage)
		if !ok || msg.Identifier != Carrier {
			return pk, nil
		}
		c.receive(msg.Data)
	}
}

// WritePacket writes pk; a dimension change ends the client part, whose epoch it resets.
func (c *Conn) WritePacket(pk packet.Packet) error {
	if _, ok := pk.(*packet.ChangeDimension); ok {
		c.end("dimension change")
	}
	return c.Conn.WritePacket(pk)
}

// Close ends the client part with the connection.
func (c *Conn) Close() error {
	c.end("disconnected")
	return c.Conn.Close()
}

// receive handles one carrier message and passes a valid client part message on.
func (c *Conn) receive(data []byte) {
	c.mu.Lock()
	envelope, err := c.handle(data)
	if err != nil {
		c.fallBack(err)
	}
	c.mu.Unlock()
	if envelope != nil && c.s.receive != nil {
		c.s.receive(c.player, envelope.Bundle, envelope.Channel, envelope.Schema, envelope.Payload)
	}
}

// handle advances the handshake by one message and returns a valid inbound envelope.
func (c *Conn) handle(data []byte) (*Envelope, error) {
	now := c.s.now()
	if c.phase == fallenBack {
		return nil, nil
	}
	if c.phase != awaitingHello && uint64(now.Unix()) >= c.expires {
		return nil, errors.New("the grant expired")
	}
	switch c.phase {
	case awaitingHello:
		return nil, c.acceptHello(data, uint64(now.Unix()))
	case awaitingReady:
		return nil, c.acceptReady(data, uint64(now.UnixMilli()))
	}
	envelope, err := c.ingress.Receive(data, uint64(now.UnixMilli()), c.epoch, c.grant)
	if err == nil && envelope == nil {
		// The client's ingress skips these as possibly newer schema revisions; the server
		// knows the client part's exact manifest, so here they can only be a violation.
		err = errors.New("an undeclared channel schema or another world epoch")
	}
	return envelope, err
}

// control decodes a handshake message of at most a carrier message's size.
func control(data []byte) (Control, error) {
	var msg Control
	if len(data) > MaxEnvelopeBytes {
		return msg, errors.New("control message too large")
	}
	return msg, Decode(data, &msg)
}

// acceptHello checks the client's Hello and answers it with a signed Accept.
func (c *Conn) acceptHello(data []byte, nowUnix uint64) error {
	msg, err := control(data)
	if err != nil {
		return err
	}
	h := msg.Hello
	offer := &c.s.offer
	switch {
	case h == nil:
		return errors.New("expected a hello")
	case h.Version != WireVersion || h.API != APIVersion:
		return fmt.Errorf("hello version %d api %d", h.Version, h.API)
	case h.OfferDigest != c.s.digest:
		return errors.New("hello for another offer")
	case h.Subclient != 0:
		return fmt.Errorf("hello for subclient %d", h.Subclient)
	case h.Capabilities == 0:
		return errors.New("the client cannot run client parts")
	case nowUnix >= offer.ExpiresUnix:
		return errors.New("the offer expired")
	}
	for _, nonce := range []string{h.ClientChallenge, h.Connection} {
		if _, err := fixedHex(nonce, nonceBytes); err != nil {
			return fmt.Errorf("hello nonce: %w", err)
		}
	}
	challenge, err := c.s.nonce()
	if err != nil {
		return err
	}
	sessionID, err := c.s.nonce()
	if err != nil {
		return err
	}
	document, err := Sign(AcceptDomain, Accept{
		Hello:           *h,
		ServerChallenge: challenge,
		Session:         sessionID,
		Audience:        offer.Audience,
		OfferDigest:     c.s.digest,
		Revision:        offer.Revision,
		ExpiresUnix:     offer.ExpiresUnix,
	}, c.s.key)
	if err != nil {
		return err
	}
	reply, err := Encode(Control{Accept: &document})
	if err != nil {
		return err
	}
	if err := c.Conn.WritePacket(&packet.ScriptMessage{Identifier: Carrier, Data: reply}); err != nil {
		return err
	}
	c.hello, c.session, c.expires = *h, sessionID, offer.ExpiresUnix
	c.phase = awaitingReady
	return nil
}

// acceptReady checks the client's Ready against the Accept, the offer and what the bundles and
// the Hello allow, and activates the client part.
func (c *Conn) acceptReady(data []byte, nowMs uint64) error {
	msg, err := control(data)
	if err != nil {
		return err
	}
	r := msg.Ready
	offer := &c.s.offer
	switch {
	case r == nil:
		return errors.New("expected ready")
	case r.Session != c.session:
		return errors.New("ready for another session")
	case !slices.EqualFunc(r.Packages, offer.Packages, func(digest string, p PackageOffer) bool { return digest == p.Digest }):
		return errors.New("ready packages are not the offer's")
	case r.Generation != InitialBundleGeneration:
		return fmt.Errorf("ready generation %d", r.Generation)
	case len(r.Permissions) != len(offer.Packages):
		return errors.New("ready permissions are not one per bundle")
	}
	recipients := make(map[string]Recipient, len(r.Permissions))
	for id, granted := range r.Permissions {
		b := c.s.bundles[id]
		if b == nil {
			return fmt.Errorf("ready grants unknown bundle %q", id)
		}
		if granted&^(b.Manifest.Permissions&c.hello.Capabilities&offer.Scope.Permissions) != 0 {
			return fmt.Errorf("ready grants %q permissions beyond its manifest and the hello", id)
		}
		recipients[id] = Recipient{Permissions: granted, Channels: b.Manifest.Channels}
	}
	c.grant = &Grant{
		Offer:      offer,
		Session:    c.session,
		Connection: c.hello.Connection,
		Subclient:  c.hello.Subclient,
		Recipients: recipients,
	}
	c.epoch = r.WorldEpoch
	c.ingress = NewIngress(ToServer, nowMs)
	c.egress = NewRateLimit(nowMs)
	c.sequence = 1
	c.phase = active
	c.s.activate(c)
	c.s.log.Info("client part active", "player", c.IdentityData().DisplayName, "world_epoch", c.epoch)
	return nil
}

// send sends one to_client envelope if the client part is active and would take it.
func (c *Conn) send(exp, channel string, schema uint16, payload []experience.Scalar) bool {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.phase != active {
		return false
	}
	now := c.s.now()
	if uint64(now.Unix()) >= c.expires {
		c.fallBack(errors.New("the grant expired"))
		return false
	}
	recipient, ok := c.grant.Recipients[exp]
	if !ok || !recipient.Permissions.Has(PermissionMessaging) || c.sequence == ^uint64(0) {
		return false
	}
	i := slices.IndexFunc(recipient.Channels, func(ch Channel) bool { return ch.ID == channel && ch.Schema == schema })
	if i < 0 || recipient.Channels[i].Validate(payload, ToClient) != nil {
		return false
	}
	data, err := Encode(Envelope{
		Version:    WireVersion,
		Session:    c.session,
		Connection: c.hello.Connection,
		Subclient:  c.hello.Subclient,
		Bundle:     exp,
		Generation: InitialBundleGeneration,
		Channel:    channel,
		Schema:     schema,
		Sequence:   c.sequence,
		WorldEpoch: c.epoch,
		Payload:    payload,
	})
	if err != nil || c.egress.Charge(len(data), uint64(now.UnixMilli())) != nil {
		return false
	}
	if err := c.Conn.WritePacket(&packet.ScriptMessage{Identifier: Carrier, Data: data}); err != nil {
		c.fallBack(err)
		return false
	}
	c.sequence++
	return true
}

// end falls back for reason unless no handshake started, which a vanilla connection never does.
func (c *Conn) end(reason string) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.phase != awaitingHello {
		c.fallBack(errors.New(reason))
	}
}

// fallBack ends the client part for good. The caller holds c.mu.
func (c *Conn) fallBack(reason error) {
	if c.phase == fallenBack {
		return
	}
	c.phase = fallenBack
	c.grant, c.ingress = nil, nil
	c.s.deactivate(c)
	c.s.log.Info("client part fallback", "player", c.IdentityData().DisplayName, "reason", reason.Error())
}
