package coreapp

import (
	"context"
	"errors"
	"io"
	"log/slog"
	"sync"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/catalog"
	"github.com/hashimthearab/rust-mcbe/core/internal/redact"
	"github.com/hashimthearab/rust-mcbe/core/localworld"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
)

// friendHostRetry spaces attempts to host after Xbox Live or signaling fails.
const friendHostRetry = 30 * time.Second

// friendHosting hosts each running dedicated-server world to Xbox friends for as long as it runs.
type friendHosting struct {
	account *authcache.Account
	worlds  *localworld.Local
	log     *slog.Logger
	open    func(context.Context, localworld.Session) (hostedFriends, io.Closer, error)

	mu      sync.Mutex
	host    hostedFriends
	xbox    io.Closer
	session localworld.Session
}

// hostedFriends is the lifecycle and invite surface of one advertised friend session.
type hostedFriends interface {
	Invite(context.Context, string) error
	Done() <-chan struct{}
	Close() error
}

// Invite sends an Xbox Live invite to the hosted world.
func (h *friendHosting) Invite(ctx context.Context, xuid string) error {
	h.mu.Lock()
	host, session := h.host, h.session
	h.mu.Unlock()
	if host == nil || !session.Current() {
		return localworld.ErrNotOpen
	}
	return host.Invite(ctx, xuid)
}

// run follows the open world until ctx ends; a sign-out restarts the whole core.
func (h *friendHosting) run(ctx context.Context) {
	var retry <-chan time.Time
	var observed localworld.Session
	defer h.stop()
	for {
		session, running, changed := h.worlds.RunningSession()
		if session != observed {
			observed, retry = session, nil
		}
		host, hosted := h.current()
		hostable := running && session.World.Backend == localworld.BackendBDS && !h.account.Closed()
		if host != nil && (!hostable || hosted != session) {
			h.stop()
			host = nil
		}
		if !hostable {
			h.stop()
		} else if host == nil && retry == nil {
			if err := h.start(ctx, session); err != nil && !errors.Is(err, localworld.ErrNotOpen) {
				h.log.Warn("friends: cannot host local world", "error", redact.Error(err))
				retry = time.After(friendHostRetry)
			}
		}
		var dropped <-chan struct{}
		if host, _ := h.current(); host != nil {
			dropped = host.Done()
		}
		select {
		case <-ctx.Done():
			return
		case <-changed:
		case <-retry:
			retry = nil
		case <-dropped:
			h.log.Warn("friends: signaling dropped; hosting again")
			h.stop()
		}
	}
}

// start installs a host only while its original local opening remains current.
func (h *friendHosting) start(ctx context.Context, session localworld.Session) error {
	if !session.Current() {
		return localworld.ErrNotOpen
	}
	startCtx, cancel := context.WithTimeout(ctx, time.Minute)
	defer cancel()
	open := h.open
	if open == nil {
		open = h.openHost
	}
	host, xbox, err := open(startCtx, session)
	if err != nil {
		return err
	}
	if err := startCtx.Err(); err != nil {
		h.closeHost(host, xbox)
		return err
	}
	if h.account.Closed() || !session.Current() {
		h.closeHost(host, xbox)
		return localworld.ErrNotOpen
	}
	h.mu.Lock()
	h.host, h.xbox, h.session = host, xbox, session
	h.mu.Unlock()
	return nil
}

// openHost publishes a friend session whose target lookup is bound to its local opening.
func (h *friendHosting) openHost(ctx context.Context, session localworld.Session) (hostedFriends, io.Closer, error) {
	xbox, err := catalog.XboxClient(ctx, h.account)
	if err != nil {
		return nil, nil, err
	}
	user := xbox.UserInfo()
	world := session.World
	host, err := proxy.StartFriendHost(ctx, proxy.FriendHostConfig{
		Account:     h.account,
		Xbox:        xbox,
		XUID:        user.XUID,
		Gamertag:    user.GamerTag,
		World:       proxy.FriendWorld{ID: world.ID, Name: world.Name, GameMode: world.GameMode},
		LocalTarget: session.ConnectionTarget,
		Logger:      h.log,
	})
	if err != nil {
		_ = xbox.Close()
		return nil, nil, err
	}
	return host, xbox, nil
}

// current snapshots the installed host and the opening it advertises.
func (h *friendHosting) current() (hostedFriends, localworld.Session) {
	h.mu.Lock()
	defer h.mu.Unlock()
	return h.host, h.session
}

// stop withdraws the host before releasing the Xbox client it uses during cleanup.
func (h *friendHosting) stop() {
	h.mu.Lock()
	host, xbox := h.host, h.xbox
	h.host, h.xbox = nil, nil
	h.session = localworld.Session{}
	h.mu.Unlock()
	h.closeHost(host, xbox)
}

// closeHost preserves host-before-Xbox teardown for installed and superseded startups.
func (h *friendHosting) closeHost(host hostedFriends, xbox io.Closer) {
	if host == nil {
		return
	}
	if err := host.Close(); err != nil {
		h.log.Debug("friends: stop hosting", "error", redact.Error(err))
	}
	_ = xbox.Close()
}
