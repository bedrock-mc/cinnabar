package extension

import (
	"bytes"
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"strings"
)

// Sign signs the canonical bytes of value under domain, the exact inverse of Verify and Rust's
// crypto::sign: the signature covers domain followed by the payload. Ed25519 is deterministic,
// so a Go signature equals Rust's for the same key and value.
func Sign(domain string, value Message, key ed25519.PrivateKey) (SignedDocument, error) {
	payload, err := Encode(value)
	if err != nil {
		return SignedDocument{}, err
	}
	return SignedDocument{
		Payload:   hex.EncodeToString(payload),
		Signature: hex.EncodeToString(ed25519.Sign(key, append([]byte(domain), payload...))),
	}, nil
}

// Verify checks d as Rust's SignedDocument::verify does: a payload of at most limit bytes, in
// lowercase hex, signed under domain by key, which must decode into into and re-encode to the
// same bytes. It returns the payload's Digest, which names an offer.
func (d SignedDocument) Verify(key, domain string, limit int, into Document) (string, error) {
	if len(d.Payload) > limit*2 {
		return "", errors.New("signed document too large")
	}
	payload, err := unhex(d.Payload)
	if err != nil {
		return "", err
	}
	public, err := fixedHex(key, ed25519.PublicKeySize)
	if err != nil {
		return "", err
	}
	signature, err := fixedHex(d.Signature, ed25519.SignatureSize)
	if err != nil {
		return "", err
	}
	if !ed25519.Verify(public, append([]byte(domain), payload...), signature) {
		return "", errors.New("invalid signature")
	}
	if err := Decode(payload, into); err != nil {
		return "", err
	}
	canonical, err := Encode(into)
	if err != nil {
		return "", err
	}
	if !bytes.Equal(canonical, payload) {
		return "", errors.New("noncanonical document")
	}
	return Digest(payload), nil
}

// Digest is SHA-256 in lowercase hex: an offer's name over its payload, a bundle's over its
// whole .cxb.
func Digest(data []byte) string {
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}

// ParseSeed reads the text of a seed file as `cinnabar-cxb keygen` writes it: 64 lowercase hex
// digits of a raw Ed25519 seed, surrounding whitespace ignored.
func ParseSeed(text string) (ed25519.PrivateKey, error) {
	seed, err := fixedHex(strings.TrimSpace(text), ed25519.SeedSize)
	if err != nil {
		return nil, fmt.Errorf("a seed is %d lowercase hex digits: %w", 2*ed25519.SeedSize, err)
	}
	return ed25519.NewKeyFromSeed(seed), nil
}

// PublicKey is key's public key in lowercase hex, as offers and manifests name keys.
func PublicKey(key ed25519.PrivateKey) string {
	return hex.EncodeToString(key.Public().(ed25519.PublicKey))
}

// unhex decodes lowercase hex only, as Rust's crypto::unhex does.
func unhex(text string) ([]byte, error) {
	if strings.ContainsFunc(text, func(c rune) bool { return !('0' <= c && c <= '9' || 'a' <= c && c <= 'f') }) {
		return nil, errors.New("invalid hex digit")
	}
	if len(text)%2 != 0 {
		return nil, errors.New("odd hex length")
	}
	return hex.DecodeString(text)
}

// fixedHex decodes exactly size bytes of lowercase hex, as Rust's crypto::fixed_hex does.
func fixedHex(text string, size int) ([]byte, error) {
	if len(text) != 2*size {
		return nil, errors.New("incorrect hex length")
	}
	return unhex(text)
}
