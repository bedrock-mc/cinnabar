package launcher

import (
	"context"
	"crypto/ecdsa"

	"github.com/hashimthearab/rust-mcbe/core/cinnabarapi"
)

// StartCinnabarAPI owns the account's Cinnabar API connection until shutdown or sign-out.
func (s *Service) StartCinnabarAPI(ctx context.Context, onRelease func(cinnabarapi.Release)) *cinnabarapi.Client {
	var token cinnabarapi.TokenSource
	if s.cfg.Account != nil {
		token = func(ctx context.Context, key *ecdsa.PublicKey) (string, error) {
			account, err := s.source()
			if err != nil {
				return "", err
			}
			return account.MultiplayerToken(ctx, key)
		}
	}
	client := cinnabarapi.New(ctx, cinnabarapi.Options{Token: token, OnRelease: onRelease, Logger: s.logger})
	s.cinnabar = client
	return client
}
