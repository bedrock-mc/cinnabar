package store

import (
	"context"
	"errors"
	"strings"
	"time"

	"github.com/sandertv/gophertunnel/minecraft/service/marketplace"
)

const (
	maxInventoryIDs   = 20000
	maxEntitlementWin = 800
)

type inventoryCache struct {
	ids     []string
	set     map[string]struct{}
	version string
	at      time.Time
}

// sessionConfig returns the store session configuration, cached briefly.
func (c *Client) sessionConfig(ctx context.Context) (*marketplace.SessionConfig, error) {
	c.mu.Lock()
	if c.config != nil && c.cfg.Now().Sub(c.configAt) < configTTL {
		cfg := c.config
		c.mu.Unlock()
		return cfg, nil
	}
	c.mu.Unlock()
	cfg, err := c.cfg.Market.SessionConfig(ctx)
	if err != nil {
		return nil, err
	}
	c.mu.Lock()
	c.config, c.configAt = cfg, c.cfg.Now()
	if cfg.UserListsVersion != "" && c.lists == "" {
		c.lists = cfg.UserListsVersion
	}
	c.mu.Unlock()
	return cfg, nil
}

// Balances returns the account's virtual currency balances (Minecoins among them).
func (c *Client) Balances(ctx context.Context) ([]Balance, error) {
	balances, err := c.cfg.Market.Balances(ctx)
	if err != nil {
		return nil, err
	}
	out := make([]Balance, 0, len(balances))
	for _, b := range balances {
		if b.Type != "" {
			out = append(out, Balance{Currency: b.Type, Amount: b.Amount})
		}
	}
	return out, nil
}

func (c *Client) loadInventory(ctx context.Context, force bool) (*inventoryCache, error) {
	c.mu.Lock()
	if cached := c.inventory; !force && cached != nil && c.cfg.Now().Sub(cached.at) < inventoryTTL {
		c.mu.Unlock()
		return cached, nil
	}
	c.mu.Unlock()
	inventory, err := c.cfg.Market.Inventory(ctx)
	if err != nil {
		return nil, err
	}
	fresh := &inventoryCache{set: map[string]struct{}{}, at: c.cfg.Now()}
	for _, entitlement := range inventory.Entitlements {
		id := strings.ToLower(entitlement.ID)
		if !ValidOfferID(id) {
			continue
		}
		if _, dup := fresh.set[id]; dup {
			continue
		}
		fresh.set[id] = struct{}{}
		fresh.ids = append(fresh.ids, id)
		if len(fresh.ids) >= maxInventoryIDs {
			break
		}
	}
	c.mu.Lock()
	if inventory.ETag != "" {
		c.etag = inventory.ETag
	}
	fresh.version = c.etag
	c.inventory = fresh
	c.mu.Unlock()
	return fresh, nil
}

func (c *Client) invalidateInventory() {
	c.mu.Lock()
	c.inventory = nil
	c.mu.Unlock()
}

// Entitlements returns a window of the owned content ids; refresh re-reads them from the service
// instead of the short-lived cache.
func (c *Client) Entitlements(ctx context.Context, offset, limit int, refresh bool) (Entitlements, error) {
	if offset < 0 || limit < 0 {
		return Entitlements{}, ErrInvalidRequest
	}
	if limit == 0 || limit > maxEntitlementWin {
		limit = maxEntitlementWin
	}
	if refresh && offset == 0 {
		c.RefreshInventory(ctx)
	}
	inv, err := c.loadInventory(ctx, refresh && offset == 0)
	if err != nil {
		return Entitlements{}, err
	}
	out := Entitlements{Total: len(inv.ids), Offset: offset, InventoryVersion: inv.version, Owned: []string{}}
	if offset < len(inv.ids) {
		end := min(offset+limit, len(inv.ids))
		out.Owned = append(out.Owned, inv.ids[offset:end]...)
	}
	return out, nil
}

// RefreshInventory asks the service to rebuild the account's inventory. It is best effort: a failure
// leaves the next inventory read to return whatever the service has.
func (c *Client) RefreshInventory(ctx context.Context) {
	version, err := c.cfg.Market.RefreshInventory(ctx)
	if err != nil {
		return
	}
	c.mu.Lock()
	c.etag = version
	c.inventory = nil
	c.mu.Unlock()
}

// MoreOffers loads the next items of a row from its continuation token.
func (c *Client) MoreOffers(ctx context.Context, token string) (RowMore, error) {
	if !ValidContinuation(token) {
		return RowMore{}, ErrInvalidRequest
	}
	items, next, err := c.continueRow(ctx, token)
	if err != nil {
		return RowMore{}, err
	}
	more := RowMore{}
	more.Offers, _ = c.offers(items, maxRowOffers)
	if ValidContinuation(next) {
		more.Continuation = next
	}
	return more, nil
}

// continueRow loads the items after token, a row's or a search's continuation, with the inventory
// loaded so they can be marked owned.
func (c *Client) continueRow(ctx context.Context, token string) ([]marketplace.Item, string, error) {
	if _, err := c.loadInventory(ctx, false); err != nil {
		return nil, "", err
	}
	c.mu.Lock()
	version := c.etag
	c.mu.Unlock()
	return c.cfg.Market.ContinueRow(ctx, token, version)
}

// owned reports whether the offer id is in the cached inventory; false when the inventory is unknown.
func (c *Client) owned(id string) bool {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.inventory == nil {
		return false
	}
	_, ok := c.inventory.set[strings.ToLower(id)]
	return ok
}

var errNoOffer = errors.New("store: offer not found")
