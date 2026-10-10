package launcher

import (
	"context"
	"errors"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/control"
	"github.com/hashimthearab/rust-mcbe/core/store"
	"golang.org/x/oauth2"
)

func TestRetainedMarketplaceRejectsEndedAccountLifetime(t *testing.T) {
	for _, end := range []string{"close", "cancel"} {
		t.Run(end, func(t *testing.T) {
			accountCtx, cancel := context.WithCancel(context.Background())
			defer cancel()
			account := authcache.NewAccount(accountCtx, "", oauth2.StaticTokenSource(&oauth2.Token{AccessToken: "x"}), nil)
			t.Cleanup(func() { _ = account.Close() })
			f := newFixture(t, account)
			market := f.service.Marketplace()
			if got, err := f.service.source(); err != nil || got != account {
				t.Fatalf("open account = %v, %v", got, err)
			}
			if end == "close" {
				if err := account.Close(); err != nil {
					t.Fatal(err)
				}
			} else {
				cancel()
			}
			if f.service.signedOut.Load() {
				t.Fatal("test must end the account without launcher sign-out")
			}
			ctx := context.Background()
			calls := map[string]func() error{
				"source":       func() error { _, err := f.service.source(); return err },
				"realms":       func() error { _, err := f.service.Realms(ctx); return err },
				"home":         func() error { _, err := market.Home(ctx, "store"); return err },
				"search":       func() error { _, err := market.Search(ctx, store.SearchQuery{}); return err },
				"offer":        func() error { _, err := market.Offer(ctx, ""); return err },
				"balances":     func() error { _, err := market.Balances(ctx); return err },
				"entitlements": func() error { _, err := market.Entitlements(ctx, 0, 0, false); return err },
				"more offers":  func() error { _, err := market.MoreOffers(ctx, ""); return err },
				"purchase":     func() error { _, err := market.Purchase(ctx, store.PurchaseRequest{}); return err },
			}
			for name, call := range calls {
				if err := call(); !errors.Is(err, control.ErrSignedOut) {
					t.Errorf("%s after account %s = %v", name, end, err)
				}
			}
		})
	}
}
