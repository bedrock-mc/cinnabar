package authcache

import (
	"context"
	"crypto/ecdsa"

	"github.com/df-mc/go-xsapi/v2/xal/sisu"
	"github.com/df-mc/go-xsapi/v2/xal/xsts"
	"github.com/sandertv/gophertunnel/minecraft/auth"
	"golang.org/x/oauth2"
)

// accountSession is the account's SISU session, whose context-free OAuth callback reads the account's
// OAuth source. SISU serializes its own token requests; callers bound their waits through the account.
type accountSession struct {
	session *sisu.Session
	account *Account
}

func newAccountSession(account *Account, config *sisu.SessionConfig) *accountSession {
	if config.HTTPClient == nil {
		config.HTTPClient = account.http
	}
	s := &accountSession{account: account}
	s.session = auth.AndroidConfig.New(s, config)
	return s
}

func (s *accountSession) XSTSToken(ctx context.Context, relyingParty string) (*xsts.Token, error) {
	return s.session.XSTSToken(ctx, relyingParty)
}

// Token is SISU's synchronous OAuth callback; a rotation it observes is the account's own, so it keeps
// the derived state.
func (s *accountSession) Token() (*oauth2.Token, error) {
	ctx, cancel := context.WithTimeout(s.account.ctx, derivationTimeout)
	defer cancel()
	return s.account.currentOAuth(ctx, true)
}

func (s *accountSession) ProofKey() *ecdsa.PrivateKey {
	return s.session.ProofKey()
}

// Snapshot copies SISU's cached state; it waits for any SISU token request in progress.
func (s *accountSession) Snapshot() *sisu.Snapshot {
	return s.session.Snapshot()
}

func (s *accountSession) InvalidateXSTSToken(relyingParty string, rejected *xsts.Token) {
	s.session.InvalidateXSTSToken(relyingParty, rejected)
}
