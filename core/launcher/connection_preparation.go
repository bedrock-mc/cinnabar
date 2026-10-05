package launcher

import (
	"context"

	"github.com/hashimthearab/rust-mcbe/core/control"
)

// PrepareConnect warms only the selected server transport. Account-bound targets stay fresh
// until Connect, which alone selects a route and may join a Realm, friend or gathering.
func (s *Service) PrepareConnect(ctx context.Context, kind, value string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if kind == "" && value == "" {
		s.cfg.Selector.PrepareTransport("")
		return nil
	}
	if s.signedOut.Load() {
		return control.ErrSignedOut
	}
	target, err := upstreamTarget(kind, value)
	if err != nil {
		return err
	}
	if kind != control.TargetRakNet {
		if _, err := s.source(); err != nil {
			return err
		}
		target = ""
	}
	s.cfg.Selector.PrepareTransport(target)
	return nil
}
