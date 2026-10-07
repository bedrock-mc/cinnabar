package extension

import (
	"errors"
	"fmt"
	"maps"
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
// through and watches for a dimension change, which on wire v1 resets the client's world epoch.
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
	// hello, session and wire are the Hello, the session and the wire that the Accept bound.
	hello   Hello
	session string
	expires uint64
	wire    Wire
	// grant, epoch, ingress, egress and sequence are Ready's session, set when active.
	grant    *Grant
	epoch    uint64
	ingress  *Ingress
	egress   RateLimit
	sequence uint64
	// epochChanged is set by a v2 epoch control until receive tells the Experiences.
	epochChanged bool
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

// WritePacket writes pk; a dimension change ends a v1 client part, whose epoch it resets.
func (c *Conn) WritePacket(pk packet.Packet) error {
	if _, ok := pk.(*packet.ChangeDimension); ok {
		c.dimensionChanged()
	}
	return c.Conn.WritePacket(pk)
}

// Close ends the client part with the connection.
func (c *Conn) Close() error {
	c.end("disconnected")
	return c.Conn.Close()
}

// receive handles one carrier message and passes a valid client part message, or a v2 client's
// new world epoch, on.
func (c *Conn) receive(data []byte) {
	c.mu.Lock()
	envelope, err := c.handle(data)
	if err != nil {
		c.fallBack(err)
	}
	var changed []string
	if c.epochChanged && c.phase == active {
		for _, exp := range slices.Sorted(maps.Keys(c.grant.Recipients)) {
			if c.grant.Recipients[exp].Permissions.Has(PermissionMessaging) {
				changed = append(changed, exp)
			}
		}
	}
	c.epochChanged = false
	c.mu.Unlock()
	if envelope != nil && c.s.receive != nil {
		c.s.receive(c.player, envelope.Bundle, envelope.Channel, envelope.Schema, envelope.Payload)
	}
	if c.s.epoch != nil {
		for _, exp := range changed {
			c.s.epoch(c.player, exp)
		}
	}
}

// handle advances the handshake by one message and returns a valid, whole and current inbound
// envelope.
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
	nowMs := uint64(now.UnixMilli())
	skipped, stale := c.ingress.Skipped, c.ingress.Stale
	var envelope *Envelope
	var err error
	if c.wire.Version == WireVersion {
		envelope, err = c.ingress.Receive(data, nowMs, c.epoch, c.grant)
	} else if err = c.ingress.charge(len(data), nowMs); err == nil {
		// After Ready a v2 client sends one kind of control message besides its envelopes.
		if msg, controlErr := control(data); controlErr == nil {
			return nil, c.changeEpoch(msg)
		}
		envelope, err = c.ingress.take(data, c.epoch, c.grant)
	}
	switch {
	case err != nil:
	case c.ingress.Skipped != skipped:
		// The client's ingress skips these as possibly newer schema revisions; the server
		// knows the client part's exact manifest, so here they can only be a violation.
		err = errors.New("an undeclared channel schema")
	case c.ingress.Stale != stale && c.wire.Version == WireVersion:
		// A v1 client keeps one world epoch for its session; a v2 client's envelopes of an
		// epoch it has since left are dropped and counted.
		err = errors.New("another world epoch")
	}
	return envelope, err
}

// changeEpoch takes a v2 client's epoch control: its world epoch changed and its runtime kept
// running, so its later envelopes, and the server's, carry the new epoch.
func (c *Conn) changeEpoch(msg Control) error {
	e := msg.Epoch
	switch {
	case e == nil:
		return errors.New("a control message other than epoch after Ready")
	case e.Session != c.session:
		return errors.New("epoch for another session")
	case e.WorldEpoch <= c.epoch:
		return fmt.Errorf("world epoch %d after %d", e.WorldEpoch, c.epoch)
	case c.ingress.open():
		return errors.New("epoch inside a fragmented message")
	}
	c.epoch = e.WorldEpoch
	c.epochChanged = true
	c.s.log.Info("client part world epoch", "player", c.IdentityData().DisplayName, "world_epoch", c.epoch)
	return nil
}

// control decodes a handshake message of at most a carrier message's size.
func control(data []byte) (Control, error) {
	var msg Control
	if len(data) > MaxEnvelopeBytes {
		return msg, errors.New("control message too large")
	}
	return msg, Decode(data, &msg)
}

// acceptHello checks the client's Hello and answers it with a signed Accept, which selects the
// highest wire version both sides speak.
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
	wire, err := selectWire(h)
	if err != nil {
		return err
	}
	challenge, err := c.s.nonce()
	if err != nil {
		return err
	}
	sessionID, err := c.s.nonce()
	if err != nil {
		return err
	}
	accept := Accept{
		Hello:           *h,
		ServerChallenge: challenge,
		Session:         sessionID,
		Audience:        offer.Audience,
		OfferDigest:     c.s.digest,
		Revision:        offer.Revision,
		ExpiresUnix:     offer.ExpiresUnix,
	}
	if wire.Version != WireVersion {
		accept.Wire = &wire
	}
	document, err := Sign(AcceptDomain, accept, c.s.key)
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
	c.hello, c.session, c.expires, c.wire = *h, sessionID, offer.ExpiresUnix, wire
	c.phase = awaitingReady
	return nil
}

// selectWire picks the highest wire version that h and the server both speak, at the lower of
// each ceiling. A v1 Hello offers nothing and gets v1, whose Accept selects nothing.
func selectWire(h *Hello) (Wire, error) {
	if h.Wire == nil {
		return v1Wire, nil
	}
	var version uint16
	for _, v := range h.Wire.Versions {
		if WireVersion <= v && v <= MaxWireVersion {
			version = max(version, v)
		}
	}
	switch version {
	case 0:
		return Wire{}, fmt.Errorf("no common wire version in %v", h.Wire.Versions)
	case WireVersion:
		return v1Wire, nil
	}
	limits := HostLimits.lowest(h.Wire.Limits)
	if !limits.ordered() {
		return Wire{}, fmt.Errorf("unusable wire limits %+v", h.Wire.Limits)
	}
	return Wire{version, limits}, nil
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
		Wire:       c.wire,
		Recipients: recipients,
	}
	c.epoch = r.WorldEpoch
	c.ingress = NewIngress(ToServer, nowMs)
	c.egress = NewRateLimit(nowMs)
	c.sequence = 1
	c.phase = active
	c.s.activate(c)
	c.s.log.Info("client part active", "player", c.IdentityData().DisplayName, "world_epoch", c.epoch, "wire", c.wire.Version)
	return nil
}

// send sends one to_client message, in fragments when it does not fit inline, if the client part
// is active and would take it. Its fragments go together or not at all.
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
	if i < 0 || recipient.Channels[i].Validate(payload, ToClient, int(c.wire.Limits.MaxMessageBytes)) != nil {
		return false
	}
	messages, err := EncodeEnvelope(Envelope{
		Version:    c.wire.Version,
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
	}, c.wire)
	if err != nil {
		return false
	}
	sizes := make([]int, len(messages))
	for i, data := range messages {
		sizes[i] = len(data)
	}
	if c.egress.ChargeAll(sizes, uint64(now.UnixMilli())) != nil {
		return false
	}
	for _, data := range messages {
		if err := c.Conn.WritePacket(&packet.ScriptMessage{Identifier: Carrier, Data: data}); err != nil {
			c.fallBack(err)
			return false
		}
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

// dimensionChanged ends a v1 client part, whose client resets its world epoch with its runtime;
// a v2 client keeps both and sends an epoch control instead. Before a Hello there is no session
// to end.
func (c *Conn) dimensionChanged() {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.phase != awaitingHello && c.wire.Version == WireVersion {
		c.fallBack(errors.New("dimension change"))
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
