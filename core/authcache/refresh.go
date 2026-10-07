package authcache

import (
	"context"
	"encoding/hex"
	"errors"
	"io"
	"time"

	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"golang.org/x/oauth2"
)

const (
	serviceRefreshLead = 10 * time.Minute // replace the service token this long before it expires
	refreshAttempt     = 30 * time.Second // bounds one background exchange so a hung request cannot wedge refresh
	refreshRecheck     = 15 * time.Minute // longest sleep, so suspend or another process's refresh is noticed
	refreshRetryMin    = time.Minute
	refreshRetryMax    = 15 * time.Minute
)

// serviceDeviceNamespace scopes the per-account Minecraft service device ID.
var serviceDeviceNamespace = uuid.MustParse("52194710-c463-4fa4-98a9-334c1b905e01")

// CompleteSignIn exchanges the signed-in account's service token and persists it next to the Microsoft
// token at oauthPath, so a join only mints its key-bound token.
func CompleteSignIn(ctx context.Context, oauthPath string, oauth oauth2.TokenSource, diagnostics io.Writer) error {
	return completeSignIn(ctx, oauthPath, oauth, diagnostics, defaultDerivedDeps())
}

func completeSignIn(ctx context.Context, oauthPath string, oauth oauth2.TokenSource, diagnostics io.Writer, deps derivedDeps) error {
	account := newAccount(ctx, DerivedCachePath(oauthPath), oauth, diagnostics, deps)
	if account == nil {
		return errors.New("authentication: no signed-in account")
	}
	defer func() { _ = account.Close() }()
	_, err := account.ServiceToken(ctx)
	return err
}

// KeepFresh refreshes the cached service token shortly before it expires while signed in. It returns
// when ctx ends or the account closes, and at once when another KeepFresh already serves this account.
func (s *Account) KeepFresh(ctx context.Context) {
	if !s.refreshing.CompareAndSwap(false, true) {
		return
	}
	defer s.refreshing.Store(false)
	retry := refreshRetryMin
	for {
		attempt, cancel := context.WithTimeout(ctx, refreshAttempt)
		remaining, err := s.refreshServiceAhead(attempt, serviceRefreshLead)
		cancel()
		var wait time.Duration
		switch {
		case ctx.Err() != nil || s.Closed() || errors.Is(err, ErrAccountClosed) || errors.Is(err, errAccountChanged):
			return
		case err != nil:
			wait, retry = retry, min(retry*2, refreshRetryMax)
		default:
			wait, retry = remaining-serviceRefreshLead, refreshRetryMin
		}
		timer := time.NewTimer(min(max(wait, refreshRetryMin), refreshRecheck))
		select {
		case <-ctx.Done():
		case <-s.ctx.Done():
		case <-timer.C:
			continue
		}
		timer.Stop()
		return
	}
}

// refreshServiceAhead replaces the service token once it is within lead of expiry and returns how long
// the current token remains valid, on the service clock its validity uses. While the current token is
// still valid the exchange runs outside the account gate, so joins keep using it; a failed exchange
// keeps it. A token another process already refreshed is reused through the shared cache.
func (s *Account) refreshServiceAhead(ctx context.Context, lead time.Duration) (time.Duration, error) {
	if err := s.begin(); err != nil {
		return 0, err
	}
	defer s.end()
	ctx, cancel := s.operationContext(ctx)
	defer cancel()
	if err := s.prepare(ctx); err != nil {
		return 0, err
	}
	if err := s.lock(ctx); err != nil {
		return 0, err
	}
	before := s.service
	s.unlock()
	if before == nil || !before.Valid() {
		token, err := s.serviceToken(ctx)
		if err != nil {
			return 0, err
		}
		return token.Remaining(), nil
	}
	remaining := before.Remaining()
	if remaining > lead {
		return remaining, nil
	}
	env, err := s.ensureEnvironment(ctx)
	if err != nil {
		return remaining, err
	}
	if !s.exchanging.CompareAndSwap(false, true) {
		return remaining, nil
	}
	defer s.exchanging.Store(false)
	if err := s.lock(ctx); err != nil {
		return remaining, err
	}
	resets, deviceID := s.resets, s.serviceDeviceIDLocked()
	s.unlock()
	source := s.deps.services(env, sessionTickets{s}, nil, deviceID, s.sessionID)
	token, err := source.ServiceToken(ctx)
	if err != nil || token == nil || !token.Valid() {
		if ctx.Err() != nil {
			return remaining, ctx.Err()
		}
		return remaining, errors.New("authentication: refresh service credential")
	}
	return s.installServiceRefresh(ctx, resets, env, before, source, token)
}

// installServiceRefresh swaps in an exchanged token unless the account changed or a newer token won.
func (s *Account) installServiceRefresh(
	ctx context.Context,
	resets uint64,
	env *service.AuthorizationEnvironment,
	before *service.Token,
	source service.TokenSource,
	token *service.Token,
) (time.Duration, error) {
	if err := s.lock(ctx); err != nil {
		return 0, err
	}
	current := s.service
	if s.resets != resets || s.environment != env ||
		(current != nil && current != before && current.ValidUntil.After(token.ValidUntil)) {
		s.unlock()
		if current != nil && current.Valid() {
			return current.Remaining(), nil
		}
		return 0, errors.New("authentication: account changed during service refresh")
	}
	s.service, s.services = token, source
	s.serviceGen++
	s.unlock()
	s.diagnostic("refresh", "service", "expiring")
	s.publish(ctx)
	return token.Remaining(), nil
}

// serviceDeviceIDLocked returns the account's stable, undashed service device ID derived from its XUID,
// as an install keeps one device ID across restarts; "" before any token carries the XUID.
func (s *Account) serviceDeviceIDLocked() string {
	for _, token := range s.xstsTokens {
		if token == nil {
			continue
		}
		for _, info := range token.DisplayClaims.UserInfo {
			if info.XUID != "" {
				id := uuid.NewSHA1(serviceDeviceNamespace, []byte(info.XUID))
				return hex.EncodeToString(id[:])
			}
		}
	}
	return ""
}
