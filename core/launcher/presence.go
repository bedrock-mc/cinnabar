package launcher

import (
	"context"
	"github.com/df-mc/go-xsapi/v2/presence"
	"github.com/hashimthearab/rust-mcbe/core/catalog"
	"github.com/hashimthearab/rust-mcbe/core/xboxpresence"
)

// StartPresence owns the account's Xbox title presence until shutdown or sign-out.
func (s *Service) StartPresence(ctx context.Context) *xboxpresence.Worker {
	var factory xboxpresence.Factory
	if s.cfg.Account != nil {
		factory = func(ctx context.Context) (*xboxpresence.Client, error) {
			account, err := s.source()
			if err != nil {
				return nil, err
			}
			client, err := catalog.XboxClient(ctx, account)
			if err != nil {
				return nil, err
			}
			return &xboxpresence.Client{
				Update: func(ctx context.Context, request presence.TitleRequest) (*presence.UpdateResult, error) {
					return client.Presence().Update(ctx, request)
				},
				Close: client.CloseContext,
			}, nil
		}
	}
	worker := xboxpresence.New(ctx, factory, s.logger)
	s.presence = worker
	return worker
}
