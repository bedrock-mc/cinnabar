package authcache

import (
	"context"
	"crypto/ecdsa"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"net/http"
	"path/filepath"
	"sync"
	"sync/atomic"
	"time"

	"github.com/df-mc/go-playfab/v2"
	"github.com/df-mc/go-xsapi/v2"
	"github.com/df-mc/go-xsapi/v2/xal/nsal"
	"github.com/df-mc/go-xsapi/v2/xal/sisu"
	"github.com/df-mc/go-xsapi/v2/xal/xasd"
	"github.com/df-mc/go-xsapi/v2/xal/xsts"
	"github.com/google/uuid"
	"github.com/hashimthearab/rust-mcbe/core/clientplatform"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/auth"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"golang.org/x/oauth2"
)

// NewAccount returns the signed-in account's runtime: the Xbox, PlayFab and Minecraft service
// credentials every consumer shares, with proof-key-bound state persisted at path (empty disables
// persistence). The OAuth source stays authoritative: new token material makes the cache miss.
// Cache failures are optional misses reported without paths or secrets. Close ends the runtime.
func NewAccount(ctx context.Context, path string, oauth oauth2.TokenSource, diagnostics io.Writer) *Account {
	return newAccount(ctx, path, oauth, diagnostics, defaultDerivedDeps())
}

type derivedDeps struct {
	discover func(context.Context) (*service.AuthorizationEnvironment, error)
	login    func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*playfab.Client, error)
	services func(env *service.AuthorizationEnvironment, tickets service.SessionTicketSource, token *service.Token, deviceID, sessionID string) service.TokenSource
	mint     func(context.Context, *service.AuthorizationEnvironment, service.TokenSource, *ecdsa.PublicKey) (string, error)
}

func defaultDerivedDeps() derivedDeps {
	return derivedDeps{
		discover: func(ctx context.Context) (*service.AuthorizationEnvironment, error) {
			discovery, err := service.Default(ctx)
			if err != nil {
				return nil, err
			}
			env := new(service.AuthorizationEnvironment)
			if err := discovery.Environment(env); err != nil {
				return nil, err
			}
			env.HTTPClient = authHTTPClient
			return env, nil
		},
		login: func(ctx context.Context, env *service.AuthorizationEnvironment, signer xsapi.TokenAndSignaturer) (*playfab.Client, error) {
			return playfab.LoginWithXbox(ctx, env.PlayFabTitleID, signer, playfab.ClientConfig{CreateAccount: true, HTTPClient: authHTTPClient})
		},
		services: func(env *service.AuthorizationEnvironment, tickets service.SessionTicketSource, token *service.Token, deviceID, sessionID string) service.TokenSource {
			config := clientplatform.TokenConfig()
			config.Device.ID = deviceID
			config.SessionID = sessionID
			return env.ResumeTokenSource(tickets, config, token)
		},
		mint: func(ctx context.Context, env *service.AuthorizationEnvironment, source service.TokenSource, key *ecdsa.PublicKey) (string, error) {
			return minecraft.NewMultiplayerTokenSource(env, source).MultiplayerToken(ctx, key)
		},
	}
}

// Account is the per-account runtime; every method is safe for concurrent use. The gate guards
// state only briefly and is never held across a network request: each credential is derived in its
// own flight, so one hung request never delays an unrelated credential.
type Account struct {
	ctx             context.Context
	cancel          context.CancelFunc
	gate            chan struct{}
	oauthGate       chan struct{} // orders OAuth reads with the binding change each applies
	publishGate     chan struct{} // serializes bundle publication
	path            string
	diagnostics     io.Writer
	diagnosticsMu   sync.Mutex // diagnostics are written outside the gate
	oauth           oauth2.TokenSource
	binding         string
	resets          uint64 // counts resetLocked, so a derivation can tell its own OAuth rotation from a replaced account state
	serviceGen      uint64 // counts service-token changes an in-flight exchange must not overwrite
	client          string
	device          xasd.TokenSource
	deviceToken     *xasd.Token
	session         *accountSession
	xstsTokens      map[string]*xsts.Token // last XSTS token per relying party restored or returned
	environment     *service.AuthorizationEnvironment
	cachedEnv       *derivedEnvironment
	service         *service.Token
	services        service.TokenSource // native source seeded with service; rebuilt after every restore
	sessionID       string              // Session-Id every service request names for this launch
	playfab         *playfab.Client     // logged in on first need; closed only by Close
	resolver        *nsal.Resolver      // PlayFab's endpoint resolver; keeps NSAL title data for the account's life
	closed          atomic.Bool
	refreshing      atomic.Bool            // one KeepFresh per account
	exchanging      atomic.Bool            // one early service exchange per account
	persisted       string                 // fingerprint of the bundle bytes last read or written
	synced          syncedTokens           // credentials of the bundle last read or written
	rejected        map[string]*xsts.Token // XSTS tokens a relying party refused; re-evicted after every reload
	rejectedService *service.Token         // service token a service refused; dropped after every reload
	deps            derivedDeps
	http            *http.Client // sends the account's own Xbox requests

	flightMu sync.Mutex
	flights  map[string]*flight

	activeMu sync.Mutex
	active   int
	closing  bool
	idle     chan struct{}
}

var (
	_ oauth2.TokenSource               = (*Account)(nil)
	_ xsapi.TokenSource                = (*Account)(nil)
	_ minecraft.MultiplayerTokenSource = (*Account)(nil)
	_ nsal.TokenSource                 = (*Account)(nil)
	_ nsal.TokenInvalidator            = (*Account)(nil)
	_ service.TokenSource              = (*Account)(nil)
	_ service.TokenInvalidator         = (*Account)(nil)
	_ service.SessionIdentifier        = (*Account)(nil)
)

// ErrAccountClosed is returned once the account has been signed out or shut down.
var ErrAccountClosed = errors.New("authentication: account is closed")

func newAccount(ctx context.Context, path string, oauth oauth2.TokenSource, diagnostics io.Writer, deps derivedDeps) *Account {
	if oauth == nil {
		return nil
	}
	if diagnostics == nil {
		diagnostics = io.Discard
	}
	ctx, cancel := context.WithCancel(ctx)
	source := &Account{
		ctx: ctx, cancel: cancel, gate: make(chan struct{}, 1), oauthGate: make(chan struct{}, 1), publishGate: make(chan struct{}, 1), diagnostics: diagnostics,
		oauth: oauth, client: clientBinding(), sessionID: uuid.NewString(), deps: deps, xstsTokens: make(map[string]*xsts.Token), http: authHTTPClient,
	}
	source.resolver = nsal.NewResolver(source)
	defer func() {
		if source.session == nil {
			source.device = xasd.ReuseTokenSource(auth.AndroidConfig.Config.Config, nil, nil)
			source.session = newAccountSession(source, &sisu.SessionConfig{DeviceTokenSource: source.device})
		}
	}()
	tok, err := source.oauthToken(ctx)
	if err != nil || tok == nil {
		return source
	}
	source.binding = oauthBinding(tok)
	if path == "" {
		return source
	}
	path, err = filepath.Abs(path)
	if err != nil {
		return source
	}
	source.path = path
	binding, client := source.binding, source.client
	state, fingerprint, err := loadDerivedBundle(source.path)
	switch {
	case err == nil && state.OAuthBinding == binding && state.ClientBinding == client:
		if err := source.restore(state); err != nil {
			source.diagnostic("miss", "bundle", "invalid")
		} else {
			source.persisted, source.synced = fingerprint, syncedFrom(state)
			source.diagnostic("hit", "bundle", "bound")
		}
	case err == nil:
		source.diagnostic("miss", "bundle", "binding")
		source.resetLocked(binding)
	case errors.Is(err, fs.ErrNotExist):
		source.diagnostic("miss", "bundle", "missing")
	case errors.Is(err, errDerivedCacheMiss):
		source.diagnostic("miss", "bundle", "invalid")
	default:
		source.diagnostic("miss", "bundle", "unsafe")
	}
	return source
}

func (s *Account) diagnostic(event, layer, reason string) {
	s.diagnosticsMu.Lock()
	defer s.diagnosticsMu.Unlock()
	_, _ = fmt.Fprintf(s.diagnostics, "AUTH_ACCEL_CACHE event=%s layer=%s reason=%s\n", event, layer, reason)
}

// Token returns the OAuth credential only while this account is open.
func (s *Account) Token() (*oauth2.Token, error) {
	if err := s.begin(); err != nil {
		return nil, err
	}
	defer s.end()
	return s.currentOAuth(s.ctx, false)
}

func (s *Account) DeviceToken(ctx context.Context) (*xasd.Token, error) {
	if err := s.begin(); err != nil {
		return nil, err
	}
	defer s.end()
	ctx, cancel := s.operationContext(ctx)
	defer cancel()
	if err := s.prepare(ctx); err != nil {
		return nil, err
	}
	if err := s.lock(ctx); err != nil {
		return nil, err
	}
	token := s.deviceToken
	s.unlock()
	if token.Valid() {
		s.diagnostic("reuse", "device", "valid")
		return token, nil
	}
	return awaitFlight(s, ctx, "device", s.deriveDevice)
}

func (s *Account) deriveDevice(ctx context.Context) (*xasd.Token, error) {
	if err := s.lock(ctx); err != nil {
		return nil, err
	}
	device, before := s.device, s.deviceToken
	s.unlock()
	token, err := device.DeviceToken(ctx)
	if err != nil {
		return nil, err
	}
	if before != nil && before.Token == token.Token {
		s.diagnostic("reuse", "device", "valid")
		return token, nil
	}
	s.diagnostic("refresh", "device", "expired")
	if err := s.lock(ctx); err != nil {
		return nil, err
	}
	changed := s.device == device && s.deviceToken != token
	if changed {
		s.deviceToken = token
	}
	s.unlock()
	if changed {
		s.publish(ctx)
	}
	return token, nil
}

func (s *Account) ProofKey() *ecdsa.PrivateKey {
	if err := s.lock(s.ctx); err != nil {
		return nil
	}
	defer s.unlock()
	if s.closed.Load() {
		return nil
	}
	return s.device.ProofKey()
}

// XSTSToken returns the account's token for relyingParty; each relying party refreshes in its own
// flight, so a join's token never waits on another party's request.
func (s *Account) XSTSToken(ctx context.Context, relyingParty string) (*xsts.Token, error) {
	if err := s.begin(); err != nil {
		return nil, err
	}
	defer s.end()
	ctx, cancel := s.operationContext(ctx)
	defer cancel()
	if err := s.prepare(ctx); err != nil {
		return nil, err
	}
	if err := s.lock(ctx); err != nil {
		return nil, err
	}
	token := s.xstsTokens[relyingParty]
	cached := token.Valid() && !s.rejectedLocked(relyingParty, token)
	s.unlock()
	if cached {
		s.diagnostic("reuse", "xsts", "valid")
		return token, nil
	}
	return awaitFlight(s, ctx, "xsts "+relyingParty, func(ctx context.Context) (*xsts.Token, error) {
		return s.deriveXSTS(ctx, relyingParty)
	})
}

func (s *Account) deriveXSTS(ctx context.Context, relyingParty string) (*xsts.Token, error) {
	for range 3 {
		if err := s.lock(ctx); err != nil {
			return nil, err
		}
		session, device := s.session, s.device
		s.unlock()
		token, err := session.XSTSToken(ctx, relyingParty)
		if err != nil {
			return nil, err
		}
		deviceToken, deviceErr := device.DeviceToken(ctx)
		if err := s.lock(ctx); err != nil {
			return nil, err
		}
		if s.session != session {
			s.unlock() // a reset or reload replaced the session while the request ran
			continue
		}
		if s.rejectedLocked(relyingParty, token) {
			// The request overlapped an invalidation and read the refused token before SISU evicted it.
			s.unlock()
			session.InvalidateXSTSToken(relyingParty, token)
			continue
		}
		changed := s.xstsTokens[relyingParty] != token
		s.xstsTokens[relyingParty] = token
		if deviceErr == nil && s.device == device && s.deviceToken != deviceToken {
			s.deviceToken, changed = deviceToken, true
		}
		s.unlock()
		if !changed {
			s.diagnostic("reuse", "xsts", "valid")
			return token, nil
		}
		s.diagnostic("refresh", "xsts", "expired")
		s.publish(ctx)
		return token, nil
	}
	return nil, errors.New("authentication: XSTS token superseded during refresh")
}

func (s *Account) rejectedLocked(relyingParty string, token *xsts.Token) bool {
	rejected := s.rejected[relyingParty]
	return rejected != nil && token != nil && rejected.Token == token.Token
}

// InvalidateXSTSToken evicts rejected through SISU and persists the eviction so no reload, in this
// process or another, can resurrect it.
func (s *Account) InvalidateXSTSToken(relyingParty string, rejected *xsts.Token) {
	if rejected == nil || rejected.Token == "" {
		return
	}
	if s.begin() != nil {
		return
	}
	defer s.end()
	ctx, cancel := context.WithTimeout(s.ctx, 10*time.Second)
	defer cancel()
	if err := s.lock(ctx); err != nil {
		return
	}
	if s.rejected == nil {
		s.rejected = make(map[string]*xsts.Token)
	}
	s.rejected[relyingParty] = rejected
	if s.rejectedLocked(relyingParty, s.xstsTokens[relyingParty]) {
		delete(s.xstsTokens, relyingParty)
	}
	s.unlock()
	if err := s.reload(ctx); err != nil {
		return
	}
	if err := s.lock(ctx); err != nil {
		return
	}
	session := s.session
	s.unlock()
	session.InvalidateXSTSToken(relyingParty, rejected)
	s.diagnostic("invalidate", "xsts", "rejected")
	s.publish(ctx)
}

// MultiplayerToken mints a key-bound multiplayer token from the shared service token.
func (s *Account) MultiplayerToken(ctx context.Context, key *ecdsa.PublicKey) (string, error) {
	if err := s.begin(); err != nil {
		return "", err
	}
	defer s.end()
	ctx, cancel := s.operationContext(ctx)
	defer cancel()
	if key == nil {
		return "", errors.New("authentication: connection proof key is absent")
	}
	env, err := s.Environment(ctx)
	if err != nil {
		return "", err
	}
	jwt, err := s.deps.mint(ctx, env, s, key)
	if err != nil {
		if ctx.Err() != nil {
			return "", ctx.Err()
		}
		return "", errors.New("authentication: mint multiplayer credential")
	}
	return jwt, nil
}

// SessionID returns the Session-Id the account's service requests send; it is fixed for the account's life.
func (s *Account) SessionID() string { return s.sessionID }

// ServiceToken returns the account's Minecraft service token from the shared native source,
// persisting it so other processes reuse it.
func (s *Account) ServiceToken(ctx context.Context) (*service.Token, error) {
	if err := s.begin(); err != nil {
		return nil, err
	}
	defer s.end()
	ctx, cancel := s.operationContext(ctx)
	defer cancel()
	if err := s.prepare(ctx); err != nil {
		return nil, err
	}
	return s.serviceToken(ctx)
}

// serviceToken returns the valid shared service token, or the one its flight exchanges.
func (s *Account) serviceToken(ctx context.Context) (*service.Token, error) {
	if _, err := s.ensureEnvironment(ctx); err != nil {
		return nil, err
	}
	if err := s.lock(ctx); err != nil {
		return nil, err
	}
	// Without a native source the token came from disk, whose claims the source must rebuild first.
	token, source := s.service, s.services
	s.unlock()
	if source != nil && token != nil && token.Valid() {
		s.diagnostic("reuse", "service", "valid")
		return token, nil
	}
	return awaitFlight(s, ctx, "service", s.exchangeService)
}

// exchangeService refreshes the shared service token against the current account state; a failure
// keeps the current token.
func (s *Account) exchangeService(ctx context.Context) (*service.Token, error) {
	for range 3 {
		env, err := s.ensureEnvironment(ctx)
		if err != nil {
			return nil, err
		}
		if err := s.lock(ctx); err != nil {
			return nil, err
		}
		source, seed, resets, gen := s.services, s.service, s.resets, s.serviceGen
		if s.environment != env {
			s.unlock()
			continue
		}
		if source != nil && seed != nil && seed.Valid() {
			s.unlock()
			return seed, nil
		}
		if source == nil {
			source = s.deps.services(env, sessionTickets{s}, seed, s.serviceDeviceIDLocked(), s.sessionID)
		}
		s.unlock()
		token, err := source.ServiceToken(ctx)
		if err != nil || token == nil || !token.Valid() {
			if ctx.Err() != nil {
				return nil, ctx.Err()
			}
			return nil, credentialError(err, "refresh service credential")
		}
		if err := s.lock(ctx); err != nil {
			return nil, err
		}
		if s.resets != resets || s.serviceGen != gen || s.environment != env || s.rejectedServiceLocked(token) {
			s.unlock() // superseded or refused meanwhile: derive against the current state
			continue
		}
		current := s.service
		if current != nil && current != seed && current.Valid() && current.ValidUntil.After(token.ValidUntil) {
			s.unlock()
			return current, nil // another refresh won with a fresher token
		}
		s.service, s.services = token, source
		s.serviceGen++
		s.unlock()
		if seed != nil && token.AuthorizationHeader == seed.AuthorizationHeader {
			s.diagnostic("reuse", "service", "valid")
			return token, nil
		}
		s.diagnostic("refresh", "service", "expired")
		s.publish(ctx)
		return token, nil
	}
	return nil, errors.New("authentication: account changed during service refresh")
}

func (s *Account) rejectedServiceLocked(token *service.Token) bool {
	return s.rejectedService != nil && token != nil && token.AuthorizationHeader == s.rejectedService.AuthorizationHeader
}

// InvalidateServiceToken drops a service token a service refused and persists the eviction.
func (s *Account) InvalidateServiceToken(rejected *service.Token) {
	if rejected == nil {
		return
	}
	if s.begin() != nil {
		return
	}
	defer s.end()
	ctx, cancel := context.WithTimeout(s.ctx, 10*time.Second)
	defer cancel()
	if err := s.reload(ctx); err != nil {
		return
	}
	if err := s.lock(ctx); err != nil {
		return
	}
	s.rejectedService = rejected
	// The native source only ever caches s.service, so dropping both evicts the rejected token.
	if s.service != nil && s.service.AuthorizationHeader == rejected.AuthorizationHeader {
		s.service = nil
		s.services = nil
		s.serviceGen++
	}
	s.unlock()
	s.diagnostic("invalidate", "service", "rejected")
	s.publish(ctx)
}

// Environment returns the discovered authorization environment.
func (s *Account) Environment(ctx context.Context) (*service.AuthorizationEnvironment, error) {
	if err := s.begin(); err != nil {
		return nil, err
	}
	defer s.end()
	ctx, cancel := s.operationContext(ctx)
	defer cancel()
	if err := s.prepare(ctx); err != nil {
		return nil, err
	}
	return s.ensureEnvironment(ctx)
}

// PlayFab returns the account's shared PlayFab client, logging in on first use; the account owns it.
func (s *Account) PlayFab(ctx context.Context) (*playfab.Client, error) {
	if err := s.begin(); err != nil {
		return nil, err
	}
	defer s.end()
	ctx, cancel := s.operationContext(ctx)
	defer cancel()
	if err := s.prepare(ctx); err != nil {
		return nil, err
	}
	return s.playFab(ctx)
}

// Closed reports whether shutdown, cancellation or a replaced sign-in ended this runtime.
func (s *Account) Closed() bool { return s.ctx.Err() != nil }

// Close cancels account operations, waits for those already running, ends the PlayFab session and
// refuses further calls.
func (s *Account) Close() error {
	s.cancel()
	s.drain()
	s.gate <- struct{}{}
	defer s.unlock()
	s.closed.Store(true)
	s.services = nil
	return s.closePlayFabLocked()
}

func (s *Account) ensureEnvironment(ctx context.Context) (*service.AuthorizationEnvironment, error) {
	if err := s.lock(ctx); err != nil {
		return nil, err
	}
	env := s.environment
	s.unlock()
	if env != nil {
		return env, nil
	}
	return awaitFlight(s, ctx, "environment", s.discoverEnvironment)
}

func (s *Account) discoverEnvironment(ctx context.Context) (*service.AuthorizationEnvironment, error) {
	if err := s.lock(ctx); err != nil {
		return nil, err
	}
	existing := s.environment
	s.unlock()
	if existing != nil {
		return existing, nil
	}
	env, err := s.deps.discover(ctx)
	if err != nil {
		if ctx.Err() != nil {
			return nil, ctx.Err()
		}
		return nil, errors.New("authentication: discover service environment")
	}
	if !validEnvironment(env) {
		return nil, errors.New("authentication: invalid service environment")
	}
	fresh := snapshotEnvironment(env)
	if err := s.lock(ctx); err != nil {
		return nil, err
	}
	defer s.unlock()
	if !sameEnvironment(s.cachedEnv, fresh) {
		s.service = nil
		s.services = nil
		s.serviceGen++
		s.diagnostic("miss", "service", "environment")
	}
	s.environment = env
	s.cachedEnv = fresh
	return env, nil
}

func (s *Account) playFab(ctx context.Context) (*playfab.Client, error) {
	if err := s.lock(ctx); err != nil {
		return nil, err
	}
	client := s.playfab
	s.unlock()
	if client != nil {
		return client, nil
	}
	env, err := s.ensureEnvironment(ctx)
	if err != nil {
		return nil, err
	}
	return awaitFlight(s, ctx, "playfab", func(ctx context.Context) (*playfab.Client, error) {
		return s.loginPlayFab(ctx, env)
	})
}

func (s *Account) loginPlayFab(ctx context.Context, env *service.AuthorizationEnvironment) (*playfab.Client, error) {
	if err := s.lock(ctx); err != nil {
		return nil, err
	}
	existing := s.playfab
	s.unlock()
	if existing != nil {
		return existing, nil // an earlier flight logged in after this caller looked
	}
	client, err := s.deps.login(ctx, env, s.resolver)
	if err != nil {
		if ctx.Err() != nil {
			return nil, ctx.Err()
		}
		return nil, credentialError(err, "PlayFab login")
	}
	if err := s.lock(ctx); err != nil {
		_ = client.Close()
		return nil, err
	}
	defer s.unlock()
	if s.playfab != nil {
		_ = client.Close()
		return s.playfab, nil
	}
	// A source can detect account replacement outside the account gate during SISU refresh.
	context.AfterFunc(s.ctx, func() { _ = client.Close() })
	s.playfab = client
	return client, nil
}

func (s *Account) closePlayFabLocked() error {
	if s.playfab == nil {
		return nil
	}
	err := s.playfab.Close()
	s.playfab = nil
	return err
}

// sessionTickets hands the native service-token source the account's shared PlayFab session.
type sessionTickets struct{ account *Account }

func (t sessionTickets) SessionTicket(ctx context.Context) (string, error) {
	client, err := t.account.playFab(ctx)
	if err != nil {
		return "", err
	}
	return client.SessionTicket(ctx)
}

// lock serializes account state while allowing queued callers to cancel their wait.
func (s *Account) lock(ctx context.Context) error {
	if s.closed.Load() {
		return ErrAccountClosed
	}
	select {
	case s.gate <- struct{}{}:
		if s.closed.Load() {
			s.unlock()
			return ErrAccountClosed
		}
		return nil
	case <-ctx.Done():
		if s.closed.Load() {
			return ErrAccountClosed
		}
		return ctx.Err()
	}
}

// unlock lets the next queued account operation access the protected state.
func (s *Account) unlock() { <-s.gate }

// operationContext ends a caller's work when either it or the account closes.
func (s *Account) operationContext(ctx context.Context) (context.Context, context.CancelFunc) {
	ctx, cancel := context.WithCancel(ctx)
	stop := context.AfterFunc(s.ctx, cancel)
	if s.ctx.Err() != nil {
		cancel()
	}
	return ctx, func() { stop(); cancel() }
}

// oauthToken propagates cancellation into cache leases. An already-running
// refresh may still finish and persist its rotation before cancellation is returned.
func (s *Account) oauthToken(ctx context.Context) (*oauth2.Token, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	var token *oauth2.Token
	var err error
	if cached, ok := s.oauth.(*persistingSource); ok {
		token, err = cached.token(ctx)
	} else {
		token, err = s.oauth.Token()
	}
	if errors.Is(err, errAccountChanged) {
		s.closed.Store(true)
		s.cancel()
		return nil, err
	}
	if ctx.Err() != nil {
		return nil, ctx.Err()
	}
	return token, err
}

// currentOAuth returns the OAuth credential and applies its binding: adopt records a rotation the
// account's own SISU refresh made, otherwise new token material resets the derived state.
func (s *Account) currentOAuth(ctx context.Context, adopt bool) (*oauth2.Token, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	select {
	case s.oauthGate <- struct{}{}:
	case <-ctx.Done():
		return nil, ctx.Err()
	}
	defer func() { <-s.oauthGate }()
	token, err := s.oauthToken(ctx)
	if err != nil || token == nil {
		if errors.Is(err, errAccountChanged) {
			return nil, err
		}
		if ctx.Err() != nil {
			return nil, ctx.Err()
		}
		return nil, errors.New("authentication: validate OAuth credential")
	}
	if err := s.lock(ctx); err != nil {
		return nil, err
	}
	defer s.unlock()
	if binding := oauthBinding(token); binding != s.binding {
		if adopt {
			s.binding = binding
		} else {
			s.resetLocked(binding)
		}
	}
	return token, nil
}

// prepare applies the OAuth binding and adopts state another process published.
func (s *Account) prepare(ctx context.Context) error {
	if _, err := s.currentOAuth(ctx, false); err != nil {
		return err
	}
	return s.reload(ctx)
}

func (s *Account) resetLocked(binding string) {
	s.resets++
	s.serviceGen++
	var proofKey *ecdsa.PrivateKey
	if s.device != nil {
		proofKey = s.device.ProofKey()
	}
	s.binding = binding
	s.environment = nil
	s.cachedEnv = nil
	s.service = nil
	s.services = nil // the PlayFab session is kept: a reset is an OAuth rotation of this process's account
	s.deviceToken = nil
	s.rejected = nil
	s.rejectedService = nil
	s.synced = syncedTokens{}
	s.xstsTokens = make(map[string]*xsts.Token)
	s.device = xasd.ReuseTokenSource(auth.AndroidConfig.Config.Config, nil, proofKey)
	s.session = newAccountSession(s, &sisu.SessionConfig{DeviceTokenSource: s.device})
	s.persisted = ""
}
