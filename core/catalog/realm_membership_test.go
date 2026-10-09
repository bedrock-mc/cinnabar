package catalog

import (
	"context"
	"errors"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft/realms"
)

// membershipFixture distinguishes read-only previews from mutation requests.
type membershipFixture struct {
	previews, accepts int
	err               error
}

// Realm returns a fixed preview and records that it did not accept membership.
func (f *membershipFixture) Realm(_ context.Context, code string) (realms.Realm, error) {
	f.previews++
	return realms.Realm{ID: 7, Name: code, State: "OPEN"}, f.err
}

// AcceptRealmInviteCode returns a fixed accepted Realm.
func (f *membershipFixture) AcceptRealmInviteCode(_ context.Context, code string) (realms.Realm, error) {
	f.accepts++
	return realms.Realm{ID: 7, Name: code, State: "OPEN"}, f.err
}

func TestMembershipPreviewDoesNotAcceptAndAcceptanceDoesNotConnect(t *testing.T) {
	f := &membershipFixture{}
	preview, err := realmMembership(context.Background(), f, " https://realms.gg/fixture-code ", false)
	if err != nil || preview.Target != "realm_id/7" || f.previews != 1 || f.accepts != 0 {
		t.Fatalf("preview: %+v %v %+v", preview, err, f)
	}
	joined, err := realmMembership(context.Background(), f, "fixture-code", true)
	if err != nil || !joined.Member || f.accepts != 1 || f.previews != 1 {
		t.Fatalf("acceptance: %+v %v %+v", joined, err, f)
	}
}

func TestMembershipRejectsUnsafeEmptyAndCancelledRequests(t *testing.T) {
	f := &membershipFixture{}
	for _, input := range []string{"", "   ", "https://realms.gg/", "../", "code?x=1", "code%2fextra", "code\\extra"} {
		if _, err := realmMembership(context.Background(), f, input, true); err == nil {
			t.Fatalf("admitted %q", input)
		}
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := realmMembership(ctx, f, "fixture", true); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	if f.accepts != 0 || f.previews != 0 {
		t.Fatalf("invalid requests reached service: %+v", f)
	}
	f.err = errors.New("service failed")
	if _, err := realmMembership(context.Background(), f, "fixture", false); !errors.Is(err, f.err) {
		t.Fatal(err)
	}
}
