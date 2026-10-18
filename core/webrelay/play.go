package webrelay

import (
	"context"
	"net"

	"github.com/hashimthearab/rust-mcbe/core/proxy"
)

// sessionPlay owns one handler run and signals when its teardown finishes.
type sessionPlay struct {
	controlCtx context.Context
	done       chan struct{}
}

// beginPlay claims the sole play connection after a cancelled controller or an older join finishes.
// Both the caller and its controller may cancel the wait or prevent a late claim.
func (session *webSession) beginPlay(ctx, controlCtx context.Context, join uint64) (*proxy.SessionHandler, error) {
	for {
		session.mu.Lock()
		if err := ctx.Err(); err != nil {
			session.mu.Unlock()
			return nil, err
		}
		if err := controlCtx.Err(); err != nil {
			session.mu.Unlock()
			return nil, err
		}
		if session.closed {
			session.mu.Unlock()
			return nil, net.ErrClosed
		}
		if session.cancelSignIn != nil || session.play != nil && session.play.controlCtx.Err() == nil &&
			(join == 0 || session.packControl == controlCtx && join <= session.packJoin) {
			session.mu.Unlock()
			return nil, errSessionBusy
		}
		if session.play == nil {
			session.play = &sessionPlay{controlCtx: controlCtx, done: make(chan struct{})}
			handler := session.handler
			session.mu.Unlock()
			return handler, nil
		}
		done := session.play.done
		session.mu.Unlock()
		select {
		case <-done:
		case <-ctx.Done():
			return nil, ctx.Err()
		case <-controlCtx.Done():
			return nil, controlCtx.Err()
		}
	}
}

// endPlay releases the sole play connection after its handler finishes and wakes resumed callers.
func (session *webSession) endPlay() {
	session.mu.Lock()
	close(session.play.done)
	session.play = nil
	session.mu.Unlock()
}

// claimPackJoin rejects reused identities within one controller before publishing any archive.
func (session *webSession) claimPackJoin(control context.Context, join uint64) bool {
	session.mu.Lock()
	defer session.mu.Unlock()
	if session.packControl != control {
		session.packControl, session.packJoin = control, 0
	}
	if join == 0 || join <= session.packJoin {
		return false
	}
	session.packJoin = join
	return true
}
