package launcher

import (
	"context"
	"errors"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/catalog"
	"github.com/hashimthearab/rust-mcbe/core/control"
)

func TestRealmMembershipNeedsAnAccountAndNeverSelectsAConnection(t *testing.T) {
	for _, signedIn := range []bool{false, true} {
		var account *authcache.Account
		if signedIn {
			account = testAccount()
			t.Cleanup(func() { _ = account.Close() })
		}
		f := newFixture(t, account)
		f.selector.Set("fixture.test:19132")
		calls := 0
		f.service.cfg.RealmMembership = func(_ context.Context, _ *authcache.Account, code string, accept bool) (catalog.Realm, error) {
			calls++
			if code != "fixture-code" || !accept {
				t.Fatalf("request: %q %v", code, accept)
			}
			return catalog.Realm{Name: "Fixture", Target: "realm_id/7"}, nil
		}
		_, err := f.service.RealmMembership(context.Background(), "https://realms.gg/fixture-code", true)
		if signedIn && err != nil || !signedIn && !errors.Is(err, control.ErrSignedOut) {
			t.Fatal(err)
		}
		if signedIn && calls != 1 || !signedIn && calls != 0 {
			t.Fatalf("calls=%d", calls)
		}
		if target, _ := f.selector.Target(); target != "fixture.test:19132" {
			t.Fatalf("selected %q", target)
		}
	}
}

func TestRealmMembershipDiscardsAccountRetiredWhileRequestRuns(t *testing.T) {
	account := testAccount()
	t.Cleanup(func() { _ = account.Close() })
	f := newFixture(t, account)
	f.service.cfg.RealmMembership = func(context.Context, *authcache.Account, string, bool) (catalog.Realm, error) {
		f.service.signedOut.Store(true)
		return catalog.Realm{Name: "Old account", Target: "realm_id/7"}, nil
	}
	if _, err := f.service.RealmMembership(context.Background(), "fixture", false); !errors.Is(err, control.ErrSignedOut) {
		t.Fatal(err)
	}
}
