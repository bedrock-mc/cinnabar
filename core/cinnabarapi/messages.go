package cinnabarapi

import (
	"crypto/ecdsa"
	"crypto/rand"
	"crypto/sha512"
	"encoding/base64"
	"errors"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"regexp"
)

// protocolVersion is the hello's protocol field for API v1.
const protocolVersion = 1

// proofPrefix precedes the server's nonce in the signed proof.
const proofPrefix = "cinnabar-api/v1/auth:"

// Message types on the v1 WebSocket.
const (
	typeHello             = "hello"
	typeChallenge         = "challenge"
	typeProof             = "proof"
	typeWelcome           = "welcome"
	typeError             = "error"
	typeVisibility        = "visibility"
	typeLookup            = "lookup"
	typeLookupResult      = "lookup_result"
	typeRelease           = "release"
	typeEmote             = "emote"
	typeLoadout           = "loadout"
	typeCosmeticOwnership = "cosmetic_ownership"
	typeEmoteState        = "emote_state"
)

// codeRateLimited is the error code for a refused lookup.
const codeRateLimited = "rate_limited"

// keepalivePing is the text frame the server auto-answers with "pong".
const keepalivePing = "ping"

var (
	uuidPattern    = regexp.MustCompile(`^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$`)
	xuidPattern    = regexp.MustCompile(`^[0-9]{1,20}$`)
	versionPattern = regexp.MustCompile(`^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$`)
)

// maxVersionLen bounds client_version as the API does.
const maxVersionLen = 64

// maxBuildNumber is the largest integer represented exactly by the v1 JSON server.
const maxBuildNumber = 1<<53 - 1

type helloMessage struct {
	Type          string `json:"type"`
	Protocol      int    `json:"protocol"`
	ClientVersion string `json:"client_version"`
	ClientBuild   uint64 `json:"client_build,omitempty"`
	Channel       string `json:"channel"`
	Visible       bool   `json:"visible"`
	Token         string `json:"token"`
}

type proofMessage struct {
	Type      string `json:"type"`
	Signature string `json:"signature"`
}

type visibilityMessage struct {
	Type    string `json:"type"`
	Visible bool   `json:"visible"`
}

type lookupMessage struct {
	Type  string   `json:"type"`
	ID    uint32   `json:"id"`
	XUIDs []string `json:"xuids,omitempty"`
	UUIDs []string `json:"uuids,omitempty"`
}

// inbound is the union of server messages; absent fields stay zero and unknown ones are ignored.
type inbound struct {
	OwnedCosmetics  []string           `json:"owned_cosmetics"`
	LoadoutRevision uint64             `json:"loadout_revision"`
	XUID            string             `json:"xuid"`
	UUID            string             `json:"uuid"`
	EmoteID         *string            `json:"emote_id"`
	Revision        uint64             `json:"revision"`
	Slots           map[string]*string `json:"slots"`
	ElapsedMS       uint32             `json:"elapsed_ms"`
	ValidForMS      uint64             `json:"valid_for_ms"`
	parameters
	Build         uint64   `json:"build"`
	Type          string   `json:"type"`
	Nonce         string   `json:"nonce"`
	Code          string   `json:"code"`
	ID            *uint32  `json:"id"`
	Cinnabar      []string `json:"cinnabar"`
	CinnabarUUIDs []string `json:"cinnabar_uuids"`
	Channel       string   `json:"channel"`
	Version       string   `json:"version"`
	PublishedAt   string   `json:"published_at"`
}

// sign returns the unpadded base64url raw r||s ES384 signature over the proof prefix and nonce.
func sign(key *ecdsa.PrivateKey, nonce string) (string, error) {
	if nonce == "" {
		return "", errors.New("cinnabarapi: empty challenge")
	}
	digest := sha512.Sum384([]byte(proofPrefix + nonce))
	r, s, err := ecdsa.Sign(rand.Reader, key, digest[:])
	if err != nil {
		return "", err
	}
	size := (key.Curve.Params().BitSize + 7) / 8
	raw := make([]byte, 2*size)
	r.FillBytes(raw[:size])
	s.FillBytes(raw[size:])
	return base64.RawURLEncoding.EncodeToString(raw), nil
}

// ValidXUID reports whether xuid has the decimal form the API accepts.
func ValidXUID(xuid string) bool { return xuidPattern.MatchString(xuid) }

// Valid reports whether the API would accept this identity in a hello.
func (c Config) Valid() bool {
	switch c.Channel {
	case "stable", "beta", "nightly":
	default:
		return false
	}
	return c.ClientBuild <= maxBuildNumber && len(c.ClientVersion) <= maxVersionLen && versionPattern.MatchString(c.ClientVersion)
}

// ValidUUID reports whether uuid has the canonical lowercase form the API accepts.
func ValidUUID(uuid string) bool { return uuidPattern.MatchString(uuid) }

// ValidLookup checks the combined identifier bound and both wire formats.
func ValidLookup(xuids, uuids []string) bool {
	if len(xuids)+len(uuids) == 0 || len(xuids)+len(uuids) > bridgecontract.CinnabarAPIMaxLookupXUIDs {
		return false
	}
	for _, xuid := range xuids {
		if !ValidXUID(xuid) {
			return false
		}
	}
	for _, uuid := range uuids {
		if !ValidUUID(uuid) {
			return false
		}
	}
	return true
}
