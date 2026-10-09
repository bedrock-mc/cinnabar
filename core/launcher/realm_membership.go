package launcher

import (
	"context"

	"github.com/hashimthearab/rust-mcbe/core/catalog"
	"github.com/hashimthearab/rust-mcbe/core/control"
)

// RealmMembership requires the active account and rechecks it before publishing the result.
func (s *Service) RealmMembership(ctx context.Context, code string, accept bool) (catalog.Realm, error) {
	src, err := s.source()
	if err != nil {
		return catalog.Realm{}, err
	}
	code, err = catalog.RealmInviteCode(code)
	if err != nil {
		return catalog.Realm{}, control.ErrInvalidTarget
	}
	realm, err := s.cfg.RealmMembership(ctx, src, code, accept)
	if err != nil {
		return catalog.Realm{}, err
	}
	if _, err := s.source(); err != nil {
		return catalog.Realm{}, err
	}
	return realm, ctx.Err()
}
