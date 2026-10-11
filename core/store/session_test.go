package store

import (
	"context"
	"errors"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/sandertv/gophertunnel/minecraft/service/marketplace"
)

func TestSessionChecksAccountBeforeEveryCachedOperation(t *testing.T) {
	denied := errors.New("signed out")
	allowed := true
	session := NewSession(func() (*authcache.Account, error) {
		if !allowed {
			return nil, denied
		}
		return nil, nil
	})
	client := new(Client)
	session.client = client
	if got, err := session.get(context.Background()); err != nil || got != client {
		t.Fatalf("cached session = %v, %v", got, err)
	}
	allowed = false
	calls := []func() error{
		func() error { _, err := session.Home(context.Background(), marketplace.PageStoreRoot); return err },
		func() error { _, err := session.Search(context.Background(), SearchQuery{}); return err },
		func() error { _, err := session.Offer(context.Background(), ""); return err },
		func() error { _, err := session.Balances(context.Background()); return err },
		func() error { _, err := session.Entitlements(context.Background(), 0, 0, false); return err },
		func() error { _, err := session.MoreOffers(context.Background(), ""); return err },
		func() error { _, err := session.Purchase(context.Background(), PurchaseRequest{}); return err },
	}
	for i, call := range calls {
		if err := call(); !errors.Is(err, denied) {
			t.Errorf("operation %d after sign-out = %v", i, err)
		}
	}
}

func TestSessionRejectsInvalidPurchaseBeforeOpening(t *testing.T) {
	session := NewSession(func() (*authcache.Account, error) { return nil, nil })
	if _, err := session.Purchase(context.Background(), PurchaseRequest{}); !errors.Is(err, ErrInvalidRequest) {
		t.Fatalf("invalid purchase = %v", err)
	}
}
