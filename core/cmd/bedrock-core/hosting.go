package main

import (
	"context"
	"log/slog"
	"sync"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/catalog"
	"github.com/hashimthearab/rust-mcbe/core/localworld"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
)

// friendHostRetry spaces attempts to host after Xbox Live or signaling fails.
const friendHostRetry = 30 * time.Second

// friendHosting hosts each running dedicated-server world to Xbox friends for as long as it runs.
type friendHosting struct {
	account *authcache.Account
	worlds  *localworld.Manager
	target  proxy.LocalTargetFunc
	log     *slog.Logger

	mu   sync.Mutex
	host *proxy.FriendHost
	xbox interface{ Close() error }
}

// Invite sends an Xbox Live invite to the hosted world.
func (h *friendHosting) Invite(ctx context.Context, xuid string) error {
	h.mu.Lock()
	host := h.host
	h.mu.Unlock()
	if host == nil {
		return localworld.ErrNotOpen
	}
	return host.Invite(ctx, xuid)
}

// run follows the open world until ctx ends; a sign-out restarts the whole core.
func (h *friendHosting) run(ctx context.Context) {
	var retry <-chan time.Time
	defer h.stop()
	for {
		world, running, changed := h.worlds.Running()
		hostable := running && world.Backend == localworld.BackendBDS && !h.account.Closed()
		if !hostable {
			h.stop()
		} else if h.current() == nil && retry == nil {
			if err := h.start(ctx, world); err != nil {
				h.log.Warn("friends: cannot host local world", "error", err)
				retry = time.After(friendHostRetry)
			}
		}
		var dropped <-chan struct{}
		if host := h.current(); host != nil {
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

func (h *friendHosting) start(ctx context.Context, world localworld.World) error {
	startCtx, cancel := context.WithTimeout(ctx, time.Minute)
	defer cancel()
	xbox, err := catalog.XboxClient(startCtx, h.account)
	if err != nil {
		return err
	}
	user := xbox.UserInfo()
	host, err := proxy.StartFriendHost(startCtx, proxy.FriendHostConfig{
		Account:     h.account,
		Xbox:        xbox,
		XUID:        user.XUID,
		Gamertag:    user.GamerTag,
		World:       proxy.FriendWorld{ID: world.ID, Name: world.Name, GameMode: world.GameMode},
		LocalTarget: h.target,
		Logger:      h.log,
	})
	if err != nil {
		_ = xbox.Close()
		return err
	}
	h.mu.Lock()
	h.host, h.xbox = host, xbox
	h.mu.Unlock()
	return nil
}

func (h *friendHosting) current() *proxy.FriendHost {
	h.mu.Lock()
	defer h.mu.Unlock()
	return h.host
}

func (h *friendHosting) stop() {
	h.mu.Lock()
	host, xbox := h.host, h.xbox
	h.host, h.xbox = nil, nil
	h.mu.Unlock()
	if host == nil {
		return
	}
	if err := host.Close(); err != nil {
		h.log.Debug("friends: stop hosting", "error", err)
	}
	_ = xbox.Close()
}
