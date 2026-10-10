package store

import (
	"context"
	"errors"
	"fmt"
	"sync"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/catalog"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"github.com/sandertv/gophertunnel/minecraft/service/marketplace"
)

// Open returns a Client on the account's shared service token; the account owns it, so closing the
// Client releases nothing.
func Open(ctx context.Context, account *authcache.Account) (*Client, error) {
	if account == nil {
		return nil, errors.New("store: no signed-in account")
	}
	xbl, err := catalog.XboxClient(ctx, account)
	if err != nil {
		return nil, err
	}
	xuid := xbl.UserInfo().XUID
	_ = xbl.Close()
	discovery, err := service.Default(ctx)
	if err != nil {
		return nil, fmt.Errorf("store: discover services: %w", err)
	}
	env, err := account.Environment(ctx)
	if err != nil {
		return nil, fmt.Errorf("store: resolve authorization service: %w", err)
	}
	market, err := marketplace.Open(discovery, account, marketplace.Identity{XUID: xuid, TitleID: string(env.PlayFabTitleID)})
	if err != nil {
		return nil, fmt.Errorf("store: %w", err)
	}
	return NewClient(Config{Market: market})
}

// Session opens its Client on first use and serves every store call from it.
type Session struct {
	account func() (*authcache.Account, error)

	mu     sync.Mutex
	client *Client
}

// NewSession opens the current account on first use and checks it before every call.
func NewSession(account func() (*authcache.Account, error)) *Session {
	return &Session{account: account}
}

// get checks the account and opens its shared client once.
func (s *Session) get(ctx context.Context) (*Client, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	account, err := s.account()
	if err != nil {
		return nil, err
	}
	if s.client != nil {
		return s.client, nil
	}
	client, err := Open(ctx, account)
	if err != nil {
		return nil, err
	}
	s.client = client
	return client, nil
}

// Home implements the store home call.
func (s *Session) Home(ctx context.Context, page string) (Page, error) {
	c, err := s.get(ctx)
	if err != nil {
		return Page{}, err
	}
	return c.Home(ctx, page)
}

// Search implements the store search call.
func (s *Session) Search(ctx context.Context, q SearchQuery) (SearchResults, error) {
	c, err := s.get(ctx)
	if err != nil {
		return SearchResults{}, err
	}
	return c.Search(ctx, q)
}

// Offer implements the offer detail call.
func (s *Session) Offer(ctx context.Context, id string) (OfferDetail, error) {
	c, err := s.get(ctx)
	if err != nil {
		return OfferDetail{}, err
	}
	return c.Offer(ctx, id)
}

// Balances implements the currency balance call.
func (s *Session) Balances(ctx context.Context) ([]Balance, error) {
	c, err := s.get(ctx)
	if err != nil {
		return nil, err
	}
	return c.Balances(ctx)
}

// Entitlements implements the owned-content call.
func (s *Session) Entitlements(ctx context.Context, offset, limit int, refresh bool) (Entitlements, error) {
	c, err := s.get(ctx)
	if err != nil {
		return Entitlements{}, err
	}
	return c.Entitlements(ctx, offset, limit, refresh)
}

// MoreOffers implements the row continuation call.
func (s *Session) MoreOffers(ctx context.Context, token string) (RowMore, error) {
	c, err := s.get(ctx)
	if err != nil {
		return RowMore{}, err
	}
	return c.MoreOffers(ctx, token)
}

// Purchase implements the Minecoin purchase call.
func (s *Session) Purchase(ctx context.Context, r PurchaseRequest) (PurchaseResult, error) {
	// Reject invalid purchases before opening a network session, while keeping
	// the signed-out error ahead of request validation.
	if _, err := s.account(); err != nil {
		return PurchaseResult{}, err
	}
	if err := r.Validate(); err != nil {
		return PurchaseResult{}, err
	}
	c, err := s.get(ctx)
	if err != nil {
		return PurchaseResult{}, err
	}
	return c.Purchase(ctx, r)
}
