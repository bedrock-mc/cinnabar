package launcher

import "github.com/hashimthearab/rust-mcbe/core/store"

// Marketplace returns a lazy store session that checks the account before every call.
func (s *Service) Marketplace() *store.Session {
	return store.NewSession(s.source)
}
