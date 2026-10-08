package extension

import (
	"bytes"
	"crypto/ed25519"
	"encoding/hex"
	"strings"
	"testing"
)

// signRaw signs arbitrary payload bytes the way Sign signs canonical ones, so a test can produce
// a correctly signed document that is not canonical.
func signRaw(domain string, payload []byte, key ed25519.PrivateKey) SignedDocument {
	return SignedDocument{
		Payload:   hex.EncodeToString(payload),
		Signature: hex.EncodeToString(ed25519.Sign(key, append([]byte(domain), payload...))),
	}
}

// Verify refuses everything Rust's SignedDocument::verify refuses: another domain or key, a
// changed signature, non-lowercase hex, an oversized payload and validly signed bytes that are
// not the canonical encoding.
func TestVerifyRejects(t *testing.T) {
	seeds, server, _ := testKeys(t)
	payload := fixture(t, "offer_payload.json")
	var golden SignedDocument
	decodeGolden(t, "offer_signed.json", &golden)
	flipped := []byte(golden.Signature)
	flipped[0] ^= 1
	// The scope's permission set, as encoded and in reverse order.
	canonicalSet := mustEncode(t, ImplementedPermissions)
	var reversed []string
	for _, p := range ImplementedPermissions.List() {
		reversed = append([]string{`"` + p.String() + `"`}, reversed...)
	}
	reorderedSet := []byte("[" + strings.Join(reversed, ",") + "]")
	for _, c := range []struct {
		name     string
		document SignedDocument
		key      string
		domain   string
		limit    int
		want     string
	}{
		{"another domain", golden, seeds.Server.PublicKey, AcceptDomain, MaxMarkerBytes / 2, "invalid signature"},
		{"another key", golden, seeds.Publisher.PublicKey, OfferDomain, MaxMarkerBytes / 2, "invalid signature"},
		{"changed signature", SignedDocument{golden.Payload, string(flipped)}, seeds.Server.PublicKey, OfferDomain, MaxMarkerBytes / 2, ""},
		{"uppercase payload", SignedDocument{strings.ToUpper(golden.Payload), golden.Signature}, seeds.Server.PublicKey, OfferDomain, MaxMarkerBytes / 2, "hex"},
		{"odd payload", SignedDocument{golden.Payload[1:], golden.Signature}, seeds.Server.PublicKey, OfferDomain, MaxMarkerBytes / 2, "hex"},
		{"short signature", SignedDocument{golden.Payload, golden.Signature[2:]}, seeds.Server.PublicKey, OfferDomain, MaxMarkerBytes / 2, "hex"},
		{"uppercase key", golden, strings.ToUpper(seeds.Server.PublicKey), OfferDomain, MaxMarkerBytes / 2, "hex"},
		{"payload over the limit", golden, seeds.Server.PublicKey, OfferDomain, len(payload) - 1, "too large"},
		{"whitespace", signRaw(OfferDomain, append([]byte(" "), payload...), server), seeds.Server.PublicKey, OfferDomain, MaxMarkerBytes / 2, "noncanonical"},
		{"reordered set", signRaw(OfferDomain, bytes.Replace(payload, canonicalSet, reorderedSet, 1), server), seeds.Server.PublicKey, OfferDomain, MaxMarkerBytes / 2, "noncanonical"},
		{"escaped slash", signRaw(OfferDomain, bytes.Replace(payload, []byte(`https://`), []byte(`https:\/\/`), 1), server), seeds.Server.PublicKey, OfferDomain, MaxMarkerBytes / 2, "noncanonical"},
		{"another type", signRaw(OfferDomain, fixture(t, "hello_payload.json"), server), seeds.Server.PublicKey, OfferDomain, MaxMarkerBytes / 2, "field"},
	} {
		t.Run(c.name, func(t *testing.T) {
			var offer Offer
			_, err := c.document.Verify(c.key, c.domain, c.limit, &offer)
			if err == nil || !strings.Contains(err.Error(), c.want) {
				t.Fatalf("Verify: %v; want an error containing %q", err, c.want)
			}
		})
	}
	var offer Offer
	if _, err := golden.Verify(seeds.Server.PublicKey, OfferDomain, len(payload), &offer); err != nil {
		t.Fatalf("a payload of exactly the limit: %v", err)
	}
}
