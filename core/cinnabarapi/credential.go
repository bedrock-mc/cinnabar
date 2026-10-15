package cinnabarapi

import (
	"context"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"time"
)

// credential owns one bound key and token pair, used only by the connection loop.
type credential struct {
	key     *ecdsa.PrivateKey
	token   string
	expires time.Time
}

// refresh mints a new pair only when the previous token is absent or near expiry.
func (c *credential) refresh(ctx context.Context, source TokenSource, now time.Time) error {
	if c.key != nil && c.expires.After(now.Add(credentialMargin)) {
		return nil
	}
	key, err := ecdsa.GenerateKey(elliptic.P384(), rand.Reader)
	if err != nil {
		return err
	}
	ctx, cancel := context.WithTimeout(ctx, tokenTimeout)
	defer cancel()
	token, err := source(ctx, &key.PublicKey)
	if err != nil {
		return err
	}
	*c = credential{key: key, token: token, expires: tokenExpiry(token)}
	return nil
}
