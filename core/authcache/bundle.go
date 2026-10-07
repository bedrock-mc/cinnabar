package authcache

import (
	"bytes"
	"context"
	"crypto/sha256"
	"crypto/x509"
	_ "embed"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"maps"
	"net/url"
	"time"

	"github.com/df-mc/go-playfab/v2/title"
	"github.com/df-mc/go-xsapi/v2/xal/sisu"
	"github.com/df-mc/go-xsapi/v2/xal/xasd"
	"github.com/df-mc/go-xsapi/v2/xal/xsts"
	"github.com/hashimthearab/rust-mcbe/core/internal/lockfile"
	"github.com/sandertv/gophertunnel/minecraft/auth"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"golang.org/x/oauth2"
)

const (
	derivedCacheVersion = 1
)

//go:embed derived_suffix.txt
var derivedCacheSuffix string

// DerivedCachePath returns the private cache path used for authentication
// state derived from the Microsoft token at oauthPath.
func DerivedCachePath(oauthPath string) string {
	return oauthPath + derivedCacheSuffix
}

type derivedEnvironment struct {
	ServiceURI     string      `json:"service_uri"`
	Issuer         string      `json:"issuer"`
	PlayFabTitleID title.Title `json:"playfab_title_id"`
}

type derivedState struct {
	Version       int                 `json:"version"`
	OAuthBinding  string              `json:"oauth_binding"`
	ClientBinding string              `json:"client_binding"`
	Environment   *derivedEnvironment `json:"environment,omitempty"`
	DeviceToken   *xasd.Token         `json:"device_token,omitempty"`
	ProofKey      string              `json:"proof_key,omitempty"`
	SISU          *sisu.Snapshot      `json:"sisu,omitempty"`
	ServiceToken  *service.Token      `json:"service_token,omitempty"`
}

// reload adopts state another process published, holding the cache lease only for the local read.
func (s *Account) reload(ctx context.Context) error {
	lease, err := s.acquireLease(ctx)
	if err != nil || lease == nil {
		return err
	}
	defer lease.Close()
	if err := s.lock(ctx); err != nil {
		return err
	}
	defer s.unlock()
	s.reloadLocked()
	return nil
}

// acquireLease bounds waits for the optional derived cache. A miss keeps
// the account usable in memory without publishing over another process's state.
func (s *Account) acquireLease(ctx context.Context) (io.Closer, error) {
	if s.path == "" {
		return nil, nil
	}
	wait, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	lease, err := lockfile.AcquireContext(wait, s.path+cacheLockSuffix)
	if err == nil {
		return lease, nil
	}
	if ctx.Err() != nil {
		return nil, ctx.Err()
	}
	s.diagnostic("miss", "write", "unavailable")
	return nil, nil
}

// reloadLocked adopts a bundle another process published since this account last read or wrote it.
func (s *Account) reloadLocked() {
	if s.path == "" {
		return
	}
	state, fingerprint, err := loadDerivedBundle(s.path)
	if err != nil || fingerprint == s.persisted || state.OAuthBinding != s.binding || state.ClientBinding != s.client {
		return
	}
	synced := syncedFrom(state)
	s.mergeLocalLocked(state, nil)
	s.adoptLocked(state, fingerprint, synced)
}

// adoptLocked restores a bundle merged from the published one, whose own tokens synced names, and
// re-applies this account's evictions to it.
func (s *Account) adoptLocked(state *derivedState, fingerprint string, synced syncedTokens) bool {
	before := s.service
	if s.restore(state) != nil {
		return false
	}
	defer func() {
		if s.service != before {
			s.serviceGen++
		}
	}()
	s.persisted, s.synced = fingerprint, synced
	for relyingParty, token := range s.rejected {
		s.session.InvalidateXSTSToken(relyingParty, token)
		if s.rejectedLocked(relyingParty, s.xstsTokens[relyingParty]) {
			delete(s.xstsTokens, relyingParty)
		}
	}
	if s.rejectedServiceLocked(s.service) {
		s.service = nil
	}
	return true
}

// publish writes the account's derived state for other processes, waiting at most publishGrace or until
// ctx ends: SISU snapshots and the cache lease can wait on unrelated work, so the rest completes in the
// background, where Close still waits for it.
func (s *Account) publish(ctx context.Context) {
	if s.path == "" || s.begin() != nil {
		return
	}
	done := make(chan struct{})
	go func() {
		defer s.end()
		defer close(done)
		device, cancel := context.WithTimeout(s.ctx, derivationTimeout)
		s.ensureDeviceToken(device)
		cancel()
		// Detached from the account so the last publication still lands while Close waits for it.
		write, cancel := context.WithTimeout(context.WithoutCancel(s.ctx), publishTimeout)
		defer cancel()
		s.publishNow(write)
	}()
	timer := time.NewTimer(publishGrace)
	defer timer.Stop()
	select {
	case <-done:
	case <-timer.C:
	case <-ctx.Done():
	}
}

// publishNow writes the account's derived state. A bundle another process published since this account
// last read or wrote one is adopted first, so only this account's evictions override it.
func (s *Account) publishNow(ctx context.Context) {
	// One publish at a time, each snapshotting after the last, so an older snapshot never lands last.
	select {
	case s.publishGate <- struct{}{}:
		defer func() { <-s.publishGate }()
	case <-ctx.Done():
		return
	}
	for range 3 {
		if !s.publishOnce(ctx) {
			return
		}
	}
}

// publishOnce writes one snapshot; it reports a retry when a reload replaced the session meanwhile.
func (s *Account) publishOnce(ctx context.Context) (retry bool) {
	if err := s.lock(ctx); err != nil {
		return false
	}
	session := s.session
	s.unlock()
	// SISU holds its own locks across requests, so the snapshot is never taken under the gate.
	snapshot := session.Snapshot()
	lease, err := s.acquireLease(ctx)
	if err != nil || lease == nil {
		return false
	}
	defer lease.Close()
	signIn, ok := s.holdSignIn(ctx)
	if !ok {
		return false
	}
	defer signIn.Close()
	if err := s.lock(ctx); err != nil {
		return false
	}
	defer s.unlock()
	if s.session != session {
		return true
	}
	state, fingerprint, err := loadDerivedBundle(s.path)
	if err == nil && fingerprint != s.persisted && state.OAuthBinding == s.binding && state.ClientBinding == s.client {
		// Another process published since this account last synced: adopt it merged with this account's
		// fresher credentials, keeping this account's evictions.
		synced := syncedFrom(state)
		s.mergeLocalLocked(state, snapshot)
		if s.adoptLocked(state, fingerprint, synced) {
			snapshot = s.session.Snapshot() // just restored, so no SISU request holds it
		} else {
			s.persisted = fingerprint // unusable here, such as another cold start's proof key: replace it
		}
	}
	s.persistLocked(snapshot)
	return false
}

// holdSignIn takes the Microsoft cache lease, after the derived one as Remove does, and reports whether
// this account's sign-in still owns it: a sign-out or a new sign-in since must never get this bundle back.
func (s *Account) holdSignIn(ctx context.Context) (io.Closer, bool) {
	source, ok := s.oauth.(*persistingSource)
	if !ok {
		return io.NopCloser(nil), true
	}
	wait, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	lease, err := lockfile.AcquireContext(wait, source.path+cacheLockSuffix)
	if err != nil {
		return nil, false
	}
	if cached, err := load(source.path); err != nil || cached.Generation != source.generation {
		_ = lease.Close()
		return nil, false
	}
	return lease, true
}

// ensureDeviceToken fills a device token a reset cleared, since a bundle is never written without one.
func (s *Account) ensureDeviceToken(ctx context.Context) {
	if err := s.lock(ctx); err != nil {
		return
	}
	device, current := s.device, s.deviceToken
	s.unlock()
	if current != nil {
		return
	}
	token, err := device.DeviceToken(auth.WithContextClient(ctx, s.http))
	if err != nil {
		return
	}
	if err := s.lock(ctx); err != nil {
		return
	}
	if s.device == device && s.deviceToken == nil {
		s.deviceToken = token
	}
	s.unlock()
}

const (
	publishGrace   = 500 * time.Millisecond // longest a credential's caller waits for its publication
	publishTimeout = 10 * time.Second       // bounds one background publication's lease and lock waits
)

// syncedTokens names the service and XSTS tokens of the bundle last read or written.
type syncedTokens struct {
	service string
	xsts    map[string]string
}

func syncedFrom(state *derivedState) syncedTokens {
	synced := syncedTokens{xsts: make(map[string]string)}
	if state.ServiceToken != nil {
		synced.service = state.ServiceToken.AuthorizationHeader
	}
	if state.SISU != nil {
		for relyingParty, token := range state.SISU.XSTSTokens {
			if token != nil {
				synced.xsts[relyingParty] = token.Token
			}
		}
	}
	return synced
}

// mergeLocalLocked keeps, per credential, whichever of the published and local copies lasts longer.
func (s *Account) mergeLocalLocked(state *derivedState, local *sisu.Snapshot) {
	if s.deviceToken.Valid() && (state.DeviceToken == nil || s.deviceToken.NotAfter.After(state.DeviceToken.NotAfter)) {
		state.DeviceToken = s.deviceToken
	}
	if s.service != nil && s.service.Valid() && s.service.AuthorizationHeader != s.synced.service &&
		sameEnvironment(state.Environment, snapshotEnvironment(s.environment)) &&
		(state.ServiceToken == nil || s.service.ValidUntil.After(state.ServiceToken.ValidUntil)) {
		state.ServiceToken = s.service
	}
	if state.SISU == nil {
		return
	}
	if local == nil {
		local = &sisu.Snapshot{}
	}
	merged := &sisu.Snapshot{TitleToken: state.SISU.TitleToken, UserToken: state.SISU.UserToken, XSTSTokens: maps.Clone(state.SISU.XSTSTokens)}
	if local.TitleToken.Valid() && (merged.TitleToken == nil || local.TitleToken.NotAfter.After(merged.TitleToken.NotAfter)) {
		merged.TitleToken = local.TitleToken
	}
	if local.UserToken.Valid() && (merged.UserToken == nil || local.UserToken.NotAfter.After(merged.UserToken.NotAfter)) {
		merged.UserToken = local.UserToken
	}
	if merged.XSTSTokens == nil {
		merged.XSTSTokens = make(map[string]*xsts.Token)
	}
	// The gate-protected mirror also holds tokens derived after local was snapshotted.
	for _, tokens := range []map[string]*xsts.Token{local.XSTSTokens, s.xstsTokens} {
		for relyingParty, token := range tokens {
			// Only a token derived since the last sync counts; a synced one missing from the bundle was evicted.
			if published := merged.XSTSTokens[relyingParty]; token.Valid() && token.Token != s.synced.xsts[relyingParty] &&
				(published == nil || token.NotAfter.After(published.NotAfter)) {
				merged.XSTSTokens[relyingParty] = token
			}
		}
	}
	state.SISU = merged
}

func (s *Account) persistLocked(snapshot *sisu.Snapshot) {
	device, proofKey := s.deviceToken, s.device.ProofKey()
	if device == nil || proofKey == nil {
		return
	}
	if snapshot != nil {
		// A snapshot taken before an invalidation finished may still hold the refused token.
		for relyingParty, token := range snapshot.XSTSTokens {
			if s.rejectedLocked(relyingParty, token) {
				delete(snapshot.XSTSTokens, relyingParty)
			}
		}
	}
	key, err := x509.MarshalECPrivateKey(proofKey)
	if err != nil {
		return
	}
	environment := snapshotEnvironment(s.environment)
	if environment == nil {
		environment = s.cachedEnv
	}
	state := derivedState{
		Version:       derivedCacheVersion,
		OAuthBinding:  s.binding,
		ClientBinding: s.client,
		Environment:   environment,
		DeviceToken:   device,
		ProofKey:      base64.RawStdEncoding.EncodeToString(key),
		SISU:          snapshot,
		ServiceToken:  s.service,
	}
	b, err := json.Marshal(state)
	if err != nil || len(b)+1 >= maxCacheSize {
		return
	}
	b = append(b, '\n')
	fingerprint := bytesFingerprint(b)
	if fingerprint == s.persisted {
		return
	}
	if err := savePrivate(s.path, b); err != nil {
		s.diagnostic("miss", "write", "contended")
		return
	}
	s.persisted, s.synced = fingerprint, syncedFrom(&state)
}

func (s *Account) restore(state *derivedState) error {
	if state == nil || state.DeviceToken == nil || state.ProofKey == "" || state.SISU == nil {
		return errDerivedCacheMiss
	}
	der, err := base64.RawStdEncoding.DecodeString(state.ProofKey)
	if err != nil {
		return errDerivedCacheMiss
	}
	key, err := x509.ParseECPrivateKey(der)
	if err != nil || key.Curve == nil || key.Curve.Params().Name != "P-256" {
		return errDerivedCacheMiss
	}
	if s.device != nil && s.device.ProofKey() != nil {
		current := s.device.ProofKey()
		if current.Curve == nil || current.Curve.Params().Name != key.Curve.Params().Name || current.D.Cmp(key.D) != 0 {
			return errDerivedCacheMiss
		}
		key = current
	}
	var cachedEnv *derivedEnvironment
	if state.Environment != nil {
		if _, err := restoreEnvironment(state.Environment); err != nil {
			return errDerivedCacheMiss
		}
		cachedEnv = state.Environment
	}
	device := xasd.ReuseTokenSource(auth.AndroidConfig.Config.Config, state.DeviceToken, key)
	tokens := maps.Clone(state.SISU.XSTSTokens) // SISU keeps and mutates the snapshot's own map
	if tokens == nil {
		tokens = make(map[string]*xsts.Token)
	}
	session := newAccountSession(s, &sisu.SessionConfig{Snapshot: state.SISU, DeviceTokenSource: device})
	var serviceToken *service.Token
	if state.ServiceToken != nil && state.ServiceToken.Valid() && cachedEnv != nil {
		serviceToken = state.ServiceToken
	}
	s.device = device
	s.deviceToken = state.DeviceToken
	s.session = session
	s.xstsTokens = tokens
	if !sameEnvironment(snapshotEnvironment(s.environment), cachedEnv) {
		s.environment = nil // a restored environment is checked against discovery once more
	}
	s.cachedEnv = cachedEnv
	// The in-memory copy of an unchanged token keeps the service clock its validity is judged by.
	if s.service == nil || cachedEnv == nil || state.ServiceToken == nil ||
		s.service.AuthorizationHeader != state.ServiceToken.AuthorizationHeader {
		s.service = serviceToken
	}
	s.services = nil
	return nil
}

var errDerivedCacheMiss = errors.New("derived authentication cache miss")

func loadDerived(path string) (*derivedState, error) {
	state, _, err := loadDerivedBundle(path)
	return state, err
}

// loadDerivedBundle also returns the fingerprint of the exact bytes read.
func loadDerivedBundle(path string) (*derivedState, string, error) {
	b, err := loadPrivate(path, maxCacheSize)
	if err != nil {
		return nil, "", err
	}
	decoder := json.NewDecoder(bytes.NewReader(b))
	decoder.DisallowUnknownFields()
	var state derivedState
	if err := decoder.Decode(&state); err != nil {
		return nil, "", errDerivedCacheMiss
	}
	var trailing any
	if err := decoder.Decode(&trailing); !errors.Is(err, io.EOF) {
		return nil, "", errDerivedCacheMiss
	}
	if state.Version != derivedCacheVersion || state.OAuthBinding == "" || state.ClientBinding == "" {
		return nil, "", errDerivedCacheMiss
	}
	return &state, bytesFingerprint(b), nil
}

func oauthBinding(token *oauth2.Token) string {
	h := sha256.New()
	for _, value := range []string{token.AccessToken, token.RefreshToken, token.TokenType} {
		var length [8]byte
		binary.BigEndian.PutUint64(length[:], uint64(len(value)))
		_, _ = h.Write(length[:])
		_, _ = h.Write([]byte(value))
	}
	return hex.EncodeToString(h.Sum(nil))
}

func bytesFingerprint(b []byte) string {
	sum := sha256.Sum256(b)
	return hex.EncodeToString(sum[:])
}

func clientBinding() string {
	b, _ := json.Marshal(struct {
		Config   auth.Config `json:"xal"`
		Protocol string      `json:"protocol"`
		App      string      `json:"application"`
	}{auth.AndroidConfig, protocol.CurrentVersion, service.ApplicationTypeMinecraftPE})
	sum := sha256.Sum256(b)
	return hex.EncodeToString(sum[:])
}

func snapshotEnvironment(env *service.AuthorizationEnvironment) *derivedEnvironment {
	if !validEnvironment(env) {
		return nil
	}
	return &derivedEnvironment{
		ServiceURI:     env.ServiceURI.String(),
		Issuer:         env.Issuer.String(),
		PlayFabTitleID: env.PlayFabTitleID,
	}
}

func restoreEnvironment(cached *derivedEnvironment) (*service.AuthorizationEnvironment, error) {
	serviceURI, err := url.Parse(cached.ServiceURI)
	if err != nil {
		return nil, err
	}
	issuer, err := url.Parse(cached.Issuer)
	if err != nil {
		return nil, err
	}
	env := &service.AuthorizationEnvironment{ServiceURI: serviceURI, Issuer: issuer, PlayFabTitleID: cached.PlayFabTitleID}
	if !validEnvironment(env) {
		return nil, fmt.Errorf("invalid environment")
	}
	return env, nil
}

func validEnvironment(env *service.AuthorizationEnvironment) bool {
	return env != nil && validHTTPSURL(env.ServiceURI) && validHTTPSURL(env.Issuer) && env.PlayFabTitleID != ""
}

func sameEnvironment(left, right *derivedEnvironment) bool {
	return left != nil && right != nil && left.ServiceURI == right.ServiceURI && left.Issuer == right.Issuer && left.PlayFabTitleID == right.PlayFabTitleID
}

func validHTTPSURL(value *url.URL) bool {
	return value != nil && value.Scheme == "https" && value.Host != "" && value.User == nil
}
