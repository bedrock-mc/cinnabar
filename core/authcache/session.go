package authcache

import (
	"context"
	"crypto/ecdsa"

	"github.com/df-mc/go-xsapi/v2/xal/sisu"
	"github.com/df-mc/go-xsapi/v2/xal/xsts"
	"github.com/sandertv/gophertunnel/minecraft/auth"
	"golang.org/x/oauth2"
)

// accountSession carries each XSTS request's context into SISU's synchronous,
// context-free OAuth callback. PlayFab retains this session and may use it outside
// the account gate, so calls are serialized here rather than on the account.
type accountSession struct {
	session *sisu.Session
	account *Account
	gate    chan struct{}
	ctx     context.Context // set only while the gate is held, never retained after a call
}

// newAccountSession wraps a SISU session without binding it to a caller's lifetime.
func newAccountSession(account *Account, config *sisu.SessionConfig) *accountSession {
	s := &accountSession{account: account, gate: make(chan struct{}, 1)}
	s.session = auth.AndroidConfig.New(s, config)
	return s
}

// XSTSToken limits both the session wait and nested OAuth lease wait to this call.
func (s *accountSession) XSTSToken(ctx context.Context, relyingParty string) (*xsts.Token, error) {
	ctx, cancel := s.account.operationContext(ctx)
	defer cancel()
	select {
	case s.gate <- struct{}{}:
	case <-ctx.Done():
		return nil, ctx.Err()
	}
	s.ctx = ctx
	defer func() {
		s.ctx = nil
		<-s.gate
	}()
	token, err := s.session.XSTSToken(ctx, relyingParty)
	if ctx.Err() != nil {
		return nil, ctx.Err()
	}
	return token, err
}

// Token is SISU's synchronous callback, invoked only inside the gated XSTS call.
func (s *accountSession) Token() (*oauth2.Token, error) {
	return s.account.oauthToken(s.ctx)
}

// ProofKey exposes the session's stable proof key to Xbox request signing.
func (s *accountSession) ProofKey() *ecdsa.PrivateKey {
	return s.session.ProofKey()
}

// Snapshot copies SISU's cached state for account persistence.
func (s *accountSession) Snapshot() *sisu.Snapshot {
	return s.session.Snapshot()
}

// InvalidateXSTSToken delegates concurrent-safe invalidation to SISU.
func (s *accountSession) InvalidateXSTSToken(relyingParty string, rejected *xsts.Token) {
	s.session.InvalidateXSTSToken(relyingParty, rejected)
}
