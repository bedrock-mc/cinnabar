package webrelay

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"net"
	"net/netip"
	"sync"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/catalog"
	"github.com/hashimthearab/rust-mcbe/core/control"
	"github.com/hashimthearab/rust-mcbe/core/launcher"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
	"github.com/sandertv/gophertunnel/minecraft/auth"
	"golang.org/x/oauth2"
)

// sessionIDBytes is the entropy of a web session ID, which is the browser's only credential.
const sessionIDBytes = 32

// newSessionID returns a random session ID in lowercase hex.
func newSessionID() string {
	var raw [sessionIDBytes]byte
	_, _ = rand.Read(raw[:])
	return hex.EncodeToString(raw[:])
}

// errSessionBusy refuses an account change or a second play connection while one is running.
var errSessionBusy = errors.New("webrelay: session is already playing")

// webSession is one browser's state: its Microsoft account, held only in memory, and the
// handler its play connections join through. Every method is safe for concurrent use.
type webSession struct {
	id     string
	client netip.Addr
	relay  *Relay

	mu           sync.Mutex
	auth         control.AuthV1
	account      *authcache.Account
	closeAccount context.CancelFunc
	cancelSignIn context.CancelFunc
	signInSeq    uint64         // numbers sign-ins; only the latest, while cancelSignIn is set, may publish
	signIns      sync.WaitGroup // running sign-in workers
	handler      *proxy.SessionHandler
	playing      bool
	attached     int       // open control and play connections
	idleSince    time.Time // when attached last fell to zero
	closed       bool
	notify       chan struct{} // closed and replaced whenever auth changes
	// connect is the join's latest progress; connectChanged closes when it changes.
	connect        proxy.ConnectProgress
	connectChanged chan struct{}
}

func newWebSession(relay *Relay, client netip.Addr) (*webSession, error) {
	session := &webSession{
		id:             newSessionID(),
		client:         client,
		relay:          relay,
		auth:           control.AuthV1{State: control.AuthSignedOut},
		idleSince:      time.Now(),
		notify:         make(chan struct{}),
		connectChanged: make(chan struct{}),
	}
	handler, err := session.newHandler(nil)
	if err != nil {
		return nil, err
	}
	session.handler = handler
	return session, nil
}

// newHandler builds the session handler for account; nil joins offline.
func (session *webSession) newHandler(account *authcache.Account) (*proxy.SessionHandler, error) {
	device, _ := authcache.LoadDevice("")
	return proxy.NewSessionHandler(proxy.SessionHandlerConfig{
		Account:         account,
		Logger:          session.relay.logger.With("session", session.id[:8]),
		Dialer:          session.relay.dialer,
		SelectTarget:    selectTarget,
		Device:          &device,
		ConnectProgress: session.setConnect,
	})
}

// setConnect publishes the join's stage and pack download progress to every control connection.
func (session *webSession) setConnect(progress proxy.ConnectProgress) {
	session.mu.Lock()
	defer session.mu.Unlock()
	session.connect = progress
	close(session.connectChanged)
	session.connectChanged = make(chan struct{})
}

// Connect returns the join's latest progress and a channel closed at its next change.
func (session *webSession) Connect() (proxy.ConnectProgress, <-chan struct{}) {
	session.mu.Lock()
	defer session.mu.Unlock()
	return session.connect, session.connectChanged
}

// Auth returns the account state and a channel closed at its next change.
func (session *webSession) Auth() (control.AuthV1, <-chan struct{}) {
	session.mu.Lock()
	defer session.mu.Unlock()
	return session.auth, session.notify
}

// setAuthLocked publishes state to every control connection.
func (session *webSession) setAuthLocked(state control.AuthV1) {
	session.auth = state
	close(session.notify)
	session.notify = make(chan struct{})
}

// SignIn starts a device-code sign-in; the account state reports its prompt and outcome.
func (session *webSession) SignIn() error {
	session.mu.Lock()
	defer session.mu.Unlock()
	if session.closed {
		return net.ErrClosed
	}
	if session.playing {
		return errSessionBusy
	}
	if session.cancelSignIn != nil || session.auth.State == control.AuthSignedIn {
		return nil
	}
	ctx, cancel := context.WithTimeout(session.relay.ctx, session.relay.cfg.SignInTimeout)
	session.signInSeq++
	session.cancelSignIn = cancel
	session.signIns.Add(1)
	go session.runSignIn(ctx, cancel, session.signInSeq)
	return nil
}

// currentSignInLocked reports whether sign-in seq is still wanted: SignOut, a later SignIn or
// closing the session makes its prompt and outcome stale.
func (session *webSession) currentSignInLocked(seq uint64) bool {
	return session.cancelSignIn != nil && session.signInSeq == seq && !session.closed
}

func (session *webSession) runSignIn(ctx context.Context, cancel context.CancelFunc, seq uint64) {
	defer session.signIns.Done()
	defer cancel()
	token, err := session.relay.deviceFlow.Request(ctx, func(prompt *oauth2.DeviceAuthResponse) error {
		session.mu.Lock()
		defer session.mu.Unlock()
		if session.currentSignInLocked(seq) {
			session.setAuthLocked(control.AuthV1{
				State: control.AuthAwaitingCode, VerificationURI: prompt.VerificationURI, UserCode: prompt.UserCode,
			})
		}
		return nil
	})
	var account *authcache.Account
	var closeAccount context.CancelFunc
	if err == nil {
		accountCtx, stop := context.WithCancel(session.relay.ctx)
		// An empty path keeps every derived credential in memory only.
		account = authcache.NewAccount(accountCtx, "", auth.AndroidConfig.TokenSource(accountCtx, token), nil)
		closeAccount = stop
		if account == nil {
			stop()
			err = errors.New("webrelay: account unavailable")
		}
	}
	var handler *proxy.SessionHandler
	if err == nil {
		handler, err = session.newHandler(account)
		if err != nil {
			_ = account.Close()
			closeAccount()
		}
	}
	gamertag := ""
	if err == nil {
		lookup, stop := context.WithTimeout(ctx, 15*time.Second)
		gamertag, _ = catalog.Gamertag(lookup, account)
		stop()
	}

	session.mu.Lock()
	defer session.mu.Unlock()
	current := session.currentSignInLocked(seq)
	if current {
		session.cancelSignIn = nil
	}
	if !current || session.playing {
		if err == nil {
			_ = handler.Close()
			_ = account.Close()
			closeAccount()
		}
		return
	}
	if err != nil {
		session.relay.logger.Info("web sign-in did not complete", "session", session.id[:8])
		session.setAuthLocked(control.AuthV1{State: control.AuthFailed, Reason: "Microsoft sign-in did not complete."})
		return
	}
	old := session.handler
	session.handler, session.account, session.closeAccount = handler, account, closeAccount
	_ = old.Close()
	session.setAuthLocked(control.AuthV1{State: control.AuthSignedIn, Gamertag: gamertag})
}

// SignOut forgets the account and any sign-in in progress.
func (session *webSession) SignOut() error {
	session.mu.Lock()
	defer session.mu.Unlock()
	if session.playing {
		return errSessionBusy
	}
	if session.cancelSignIn != nil {
		session.cancelSignIn()
		session.cancelSignIn = nil
	}
	if session.account == nil {
		session.setAuthLocked(control.AuthV1{State: control.AuthSignedOut})
		return nil
	}
	handler, err := session.newHandler(nil)
	if err != nil {
		return err
	}
	session.dropAccountLocked()
	_ = session.handler.Close()
	session.handler = handler
	session.setAuthLocked(control.AuthV1{State: control.AuthSignedOut})
	return nil
}

// dropAccountLocked closes the account, which erases its tokens from memory.
func (session *webSession) dropAccountLocked() {
	if session.account != nil {
		_ = session.account.Close()
		session.closeAccount()
		session.account, session.closeAccount = nil, nil
	}
}

// beginPlay claims the session's only play connection and returns the handler it joins through.
func (session *webSession) beginPlay() (*proxy.SessionHandler, error) {
	session.mu.Lock()
	defer session.mu.Unlock()
	if session.closed {
		return nil, net.ErrClosed
	}
	if session.playing {
		return nil, errSessionBusy
	}
	session.playing = true
	return session.handler, nil
}

func (session *webSession) endPlay() {
	session.mu.Lock()
	session.playing = false
	session.mu.Unlock()
}

// attach counts a connection; the returned detach starts the idle clock when the last one closes.
func (session *webSession) attach() (detach func(), err error) {
	session.mu.Lock()
	defer session.mu.Unlock()
	if session.closed {
		return nil, net.ErrClosed
	}
	session.attached++
	var once sync.Once
	return func() {
		once.Do(func() {
			session.mu.Lock()
			defer session.mu.Unlock()
			if session.attached--; session.attached == 0 {
				session.idleSince = time.Now()
			}
		})
	}, nil
}

// expired reports a session with no connections for longer than ttl.
func (session *webSession) expired(now time.Time, ttl time.Duration) bool {
	session.mu.Lock()
	defer session.mu.Unlock()
	return session.attached == 0 && now.Sub(session.idleSince) > ttl
}

// close ends sign-in, erases the account and shuts the handler; play connections end with ctx.
func (session *webSession) close() {
	session.mu.Lock()
	if session.closed {
		session.mu.Unlock()
		return
	}
	session.closed = true
	if session.cancelSignIn != nil {
		session.cancelSignIn()
	}
	session.dropAccountLocked()
	handler := session.handler
	session.mu.Unlock()
	// A cancelled worker still releases any account it built before it returns.
	session.signIns.Wait()
	if err := handler.Close(); err != nil {
		session.relay.logger.Warn("web session close", "error", err)
	}
}

// selectTarget admits only a host:port RakNet target; the dialer decides whether it is reachable.
func selectTarget(_ context.Context, kind, value string) (string, error) {
	if kind != control.TargetRakNet {
		return "", control.ErrInvalidTarget
	}
	return launcher.RakNetTarget(value)
}
