// Package store serves the launcher's Marketplace screens as the signed-in account: layout pages,
// search, offer details, balance, inventory and confirmed Minecoin purchases. The store service protocol
// lives in gophertunnel's service/marketplace; this package keeps confirmation, purchase
// deduplication, caching and the bridge DTOs.
package store

import (
	"errors"
	"sync"
	"time"

	"github.com/sandertv/gophertunnel/minecraft/service/marketplace"
)

const (
	configTTL    = 10 * time.Minute
	inventoryTTL = time.Minute
)

// Config wires a Client.
type Config struct {
	Market *marketplace.Client // its environment carries the purchase identity
	Now    func() time.Time
}

// Client is the store backend; it is safe for concurrent use.
type Client struct {
	cfg   Config
	guard *purchaseGuard

	mu        sync.Mutex
	config    *marketplace.SessionConfig
	configAt  time.Time
	inventory *inventoryCache
}

// NewClient returns a Client; Market is required.
func NewClient(cfg Config) (*Client, error) {
	if cfg.Market == nil {
		return nil, errors.New("store: incomplete client configuration")
	}
	if cfg.Now == nil {
		cfg.Now = time.Now
	}
	return &Client{cfg: cfg, guard: newPurchaseGuard(cfg.Now)}, nil
}
