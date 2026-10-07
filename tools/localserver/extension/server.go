package extension

import (
	"crypto/ed25519"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net"
	"net/netip"
	"slices"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/google/uuid"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/experience"
)

// DeliveryOrigin is the origin of every offered package URL. It is a reserved name (RFC 6761)
// that never resolves, so a client takes a bundle only from its digest cache, which
// `cinnabar-cxb seed-cache` fills; the client's network policy is unchanged.
const DeliveryOrigin = "https://cxb.invalid"

// Fallback is the offer's description of what a player without the client part gets.
const Fallback = "Without the client part the server plays exactly as it does for any Bedrock client."

// offerLifetimeSecs is how long an offer lasts from startup: an hour less than the client
// accepts, so a client clock up to an hour behind the server's still takes it.
const offerLifetimeSecs = MaxOfferLifetimeSecs - 60*60

// MediaGPUBytes is the GPU memory an offer requests when a bundle may draw scene or media
// surfaces: one 1280x720 RGBA texture with headroom.
const MediaGPUBytes = 16 << 20

// nonceBytes is the size of the Accept's server challenge and session, as the client checks them.
const nonceBytes = 32

// Config is what NewServer offers: the server key that signs the offer and every Accept, the
// audience players join by, the deployment revision and the bundles.
type Config struct {
	Key ed25519.PrivateKey
	// Audience is the host:port players join by, exactly as the client canonicalizes the address
	// it was given, for example 127.0.0.1:19132.
	Audience string
	Revision uint64
	Bundles  []Bundle
	// MediaOrigins are canonical HTTPS origins added to the offer for media descriptor URLs.
	MediaOrigins []string
	// Log receives one line per client part that starts or falls back; nil discards.
	Log *slog.Logger
	// Now and Rand default to the wall clock and crypto/rand.
	Now  func() time.Time
	Rand io.Reader
}

// Server is the server half of client parts. It offers the bundles in a signed marker, wraps
// Dragonfly's listeners so that each connection runs the PR #34 handshake on the carrier, and
// carries typed channel messages between a player's active client part and the Experience whose
// id is the bundle id. A connection that breaks any rule falls back silently: its client part
// gets nothing more and its messages are ignored, while the player stays connected.
//
// It implements experience.ClientChannels. Its methods may be called from any goroutine.
type Server struct {
	key     ed25519.PrivateKey
	offer   Offer
	digest  string
	marker  Marker
	bundles map[string]*Bundle
	log     *slog.Logger
	now     func() time.Time

	randMu sync.Mutex
	rand   io.Reader

	// receive gets the valid messages of client parts; nil drops them.
	receive func(player uuid.UUID, exp, channel string, schema uint16, payload []experience.Scalar)

	mu sync.Mutex
	// active holds each player's connection whose client part is active.
	active map[uuid.UUID]*Conn
}

var _ experience.ClientChannels = (*Server)(nil)

// NewServer signs the offer of cfg.Bundles, valid from now for a day less an hour.
func NewServer(cfg Config) (*Server, error) {
	if len(cfg.Key) != ed25519.PrivateKeySize {
		return nil, errors.New("no server key")
	}
	if err := checkAudience(cfg.Audience); err != nil {
		return nil, err
	}
	s := &Server{
		key:     cfg.Key,
		bundles: make(map[string]*Bundle, len(cfg.Bundles)),
		log:     cfg.Log,
		now:     cfg.Now,
		rand:    cfg.Rand,
		active:  make(map[uuid.UUID]*Conn),
	}
	if s.log == nil {
		s.log = slog.New(slog.DiscardHandler)
	}
	if s.now == nil {
		s.now = time.Now
	}
	if s.rand == nil {
		s.rand = rand.Reader
	}
	offer, err := s.newOffer(cfg)
	if err != nil {
		return nil, err
	}
	document, err := Sign(OfferDomain, offer, s.key)
	if err != nil {
		return nil, err
	}
	payload, err := Encode(offer)
	if err != nil {
		return nil, err
	}
	s.offer, s.digest = offer, Digest(payload)
	s.marker = Marker{ServerKey: offer.ServerKey, Offer: document}
	marker, err := Encode(s.marker)
	if err != nil {
		return nil, err
	}
	if len(payload) > MaxMarkerBytes/2 || len(marker) > MaxMarkerBytes {
		return nil, fmt.Errorf("the offer does not fit the client's %d-byte marker", MaxMarkerBytes)
	}
	return s, nil
}

// newOffer is the offer of cfg.Bundles in order: their permissions, the delivery and media
// origins, as much memory as the client gives that many guests, and GPU memory for surfaces.
func (s *Server) newOffer(cfg Config) (Offer, error) {
	if len(cfg.Bundles) == 0 || len(cfg.Bundles) > MaxBundles {
		return Offer{}, fmt.Errorf("%d bundles; an offer holds 1 to %d", len(cfg.Bundles), MaxBundles)
	}
	offer := Offer{
		Version:     WireVersion,
		Audience:    cfg.Audience,
		ServerKey:   PublicKey(s.key),
		Revision:    cfg.Revision,
		ExpiresUnix: uint64(s.now().Unix()) + offerLifetimeSecs,
		Scope: Scope{
			Origins:     []string{DeliveryOrigin},
			MemoryBytes: min(MaxSessionMemory, uint64(len(cfg.Bundles))*MaxGuestMemory),
		},
		Fallback: Fallback,
		Carrier:  Carrier,
	}
	for _, origin := range cfg.MediaOrigins {
		if !slices.Contains(offer.Scope.Origins, origin) {
			offer.Scope.Origins = append(offer.Scope.Origins, origin)
		}
	}
	if len(offer.Scope.Origins) > MaxOrigins {
		return Offer{}, fmt.Errorf("%d origins; an offer holds at most %d", len(offer.Scope.Origins), MaxOrigins)
	}
	var total uint64
	for _, b := range cfg.Bundles {
		if s.bundles[b.Manifest.ID] != nil {
			return Offer{}, fmt.Errorf("bundle %q offered twice", b.Manifest.ID)
		}
		s.bundles[b.Manifest.ID] = &b
		total += b.Bytes
		offer.Scope.Permissions |= b.Manifest.Permissions
		offer.Packages = append(offer.Packages, PackageOffer{
			ID:           b.Manifest.ID,
			PublisherKey: b.Manifest.PublisherKey,
			Digest:       b.Digest,
			Bytes:        b.Bytes,
			URL:          DeliveryOrigin + "/" + b.Digest + bundleSuffix,
		})
	}
	if offer.Scope.Permissions.Has(PermissionScene) || offer.Scope.Permissions.Has(PermissionMedia) {
		offer.Scope.GPUBytes = MediaGPUBytes
	}
	if total > MaxExpandedBytes {
		return Offer{}, fmt.Errorf("bundles of %d bytes together, over the client's %d", total, MaxExpandedBytes)
	}
	return offer, nil
}

// checkAudience accepts host:port only in the form the client canonicalizes an address to: a
// lowercase host, brackets around IPv6 and an explicit nonzero port without padding.
func checkAudience(audience string) error {
	host, port, err := net.SplitHostPort(audience)
	if err == nil {
		var n uint64
		if n, err = strconv.ParseUint(port, 10, 16); err == nil && n != 0 && canonicalHost(host) &&
			net.JoinHostPort(strings.ToLower(host), strconv.FormatUint(n, 10)) == audience &&
			!strings.HasSuffix(host, ".") {
			return nil
		}
	}
	return fmt.Errorf("audience %q is not a canonical host:port such as 127.0.0.1:19132", audience)
}

// canonicalHost accepts the ASCII host form emitted by the client's URL parser. IPs must
// already use their normalized spelling; numeric final labels cannot masquerade as DNS names.
func canonicalHost(host string) bool {
	if ip, err := netip.ParseAddr(host); err == nil {
		canonical := ip.String()
		if ip.Is4In6() {
			// WHATWG URL hosts use hex IPv6 segments even for IPv4-mapped addresses.
			bytes := ip.As16()
			canonical = fmt.Sprintf("::ffff:%x:%x", uint16(bytes[12])<<8|uint16(bytes[13]), uint16(bytes[14])<<8|uint16(bytes[15]))
		}
		return ip.Zone() == "" && canonical == host
	}
	if host == "" || strings.ContainsFunc(host, func(c rune) bool {
		return !(c >= 'a' && c <= 'z' || c >= '0' && c <= '9' || c == '-' || c == '.' || c == '_')
	}) {
		return false
	}
	last := host[strings.LastIndexByte(host, '.')+1:]
	if last == "" || !strings.ContainsFunc(last, func(c rune) bool { return c < '0' || c > '9' }) {
		return false
	}
	if digits, hex := strings.CutPrefix(last, "0x"); hex && !strings.ContainsFunc(digits, func(c rune) bool {
		return !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f')
	}) {
		return false
	}
	return true
}

// Marker is the marker that the offer's resource pack holds.
func (s *Server) Marker() Marker {
	return s.marker
}

// Offer is the signed offer.
func (s *Server) Offer() Offer {
	return s.offer
}

// OnClientMessage sets the function that gets each valid message a player's client part sends,
// for the Experience exp whose id is the bundle id. It runs on that player's packet reader, so it
// must not block. Set it before the server listens.
func (s *Server) OnClientMessage(receive func(player uuid.UUID, exp, channel string, schema uint16, payload []experience.Scalar)) {
	s.receive = receive
}

// Declares reports whether the bundle of the Experience exp declares channel, revision schema,
// in the direction to the client.
func (s *Server) Declares(exp, channel string, schema uint16) bool {
	b := s.bundles[exp]
	return b != nil && slices.ContainsFunc(b.Manifest.Channels, func(c Channel) bool {
		return c.ID == channel && c.Schema == schema && c.Direction == ToClient
	})
}

// Send sends payload on channel, revision schema, to the client part of exp that player runs. It
// reports false when the player has no active client part, the bundle was not granted messaging,
// the channel is not a declared to_client channel, the record does not fit it, or the client's
// rate would be exceeded.
func (s *Server) Send(player uuid.UUID, exp, channel string, schema uint16, payload []experience.Scalar) bool {
	s.mu.Lock()
	c := s.active[player]
	s.mu.Unlock()
	return c != nil && c.send(exp, channel, schema, payload)
}

// activate makes c its player's active connection.
func (s *Server) activate(c *Conn) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.active[c.player] = c
}

// deactivate forgets c if it is its player's active connection.
func (s *Server) deactivate(c *Conn) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.active[c.player] == c {
		delete(s.active, c.player)
	}
}

// nonce is a fresh random value in lowercase hex, as the Accept's challenge and session.
func (s *Server) nonce() (string, error) {
	s.randMu.Lock()
	defer s.randMu.Unlock()
	b := make([]byte, nonceBytes)
	if _, err := io.ReadFull(s.rand, b); err != nil {
		return "", err
	}
	return hex.EncodeToString(b), nil
}
