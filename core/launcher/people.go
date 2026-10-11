package launcher

import (
	"context"

	"github.com/hashimthearab/rust-mcbe/core/catalog"
)

// People lists the account's Xbox friends; the client caches their gamerpics.
func (s *Service) People(ctx context.Context) ([]catalog.Person, error) {
	src, err := s.source()
	if err != nil {
		return nil, err
	}
	return s.cfg.People(ctx, src)
}
