package catalog

import (
	"context"
	"fmt"
	"sync"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"github.com/sandertv/gophertunnel/minecraft/service/gatherings"
)

// ExperienceCounts keeps one account's typed client so its service cache survives menu reads.
type ExperienceCounts struct {
	mu     sync.Mutex
	client *gatherings.Client
}

// Counts resolves the service once, then lets the typed client manage count refreshes.
func (c *ExperienceCounts) Counts(ctx context.Context, account *authcache.Account) ([]gatherings.ExperiencePlayerCount, error) {
	if account == nil {
		return nil, errNoAccount
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.client == nil {
		discovery, err := service.Default(ctx)
		if err != nil {
			return nil, fmt.Errorf("discover services: %w", err)
		}
		client, err := gatheringsClient(discovery, account)
		if err != nil {
			return nil, err
		}
		c.client = client
	}
	return c.client.PlayerCounts(ctx)
}
