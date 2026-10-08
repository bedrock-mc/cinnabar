package update

import (
	"crypto/ed25519"
	_ "embed"
	"strings"
)

// trustedKeys is the single public-key source shared by the client and release signer.
//
//go:embed trusted-keys.txt
var trustedKeys string

// TrustedKeys loads the public keys committed by the release owner.
func TrustedKeys() (map[string]ed25519.PublicKey, error) {
	return ParseKeys(strings.ReplaceAll(strings.TrimSpace(trustedKeys), "\n", ","))
}
