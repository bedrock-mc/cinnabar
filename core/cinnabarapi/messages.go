package cinnabarapi

import (
	"crypto/ecdsa"
	"crypto/rand"
	"crypto/sha512"
	"encoding/base64"
	"errors"
	"regexp"
)

// protocolVersion is the hello's protocol field for API v1.
const protocolVersion = 1

// proofPrefix precedes the server's nonce in the signed proof.
const proofPrefix = "cinnabar-api/v1/auth:"

// Message types on the v1 WebSocket.
const (
	typeHello        = "hello"
	typeChallenge    = "challenge"
	typeProof        = "proof"
	typeWelcome      = "welcome"
	typeError        = "error"
	typeVisibility   = "visibility"
	typeLookup       = "lookup"
	typeLookupResult = "lookup_result"
	typeRelease      = "release"
)

// codeRateLimited is the error code for a refused lookup.
const codeRateLimited = "rate_limited"

// keepalivePing is the text frame the server auto-answers with "pong".
const keepalivePing = "ping"

var xuidPattern = regexp.MustCompile(`^[0-9]{1,20}$`)

type helloMessage struct {
	Type          string `json:"type"`
	Protocol      int    `json:"protocol"`
	ClientVersion string `json:"client_version"`
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
	XUIDs []string `json:"xuids"`
}

// inbound is the union of server messages; absent fields stay zero and unknown ones are ignored.
type inbound struct {
	Type     string   `json:"type"`
	Nonce    string   `json:"nonce"`
	Code     string   `json:"code"`
	ID       *uint32  `json:"id"`
	Cinnabar []string `json:"cinnabar"`
	Channel  string   `json:"channel"`
	Version  string   `json:"version"`
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
