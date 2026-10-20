package catalog

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"strings"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/sandertv/gophertunnel/minecraft/realms"
)

// TestRealmInviteCodeConformance exercises the actual extractor with the shared native input corpus.
func TestRealmInviteCodeConformance(t *testing.T) {
	var cases []struct {
		Name  string  `json:"name"`
		Input string  `json:"input"`
		Code  *string `json:"code"`
	}
	data, err := os.ReadFile("realm_membership_conformance.json")
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(data, &cases); err != nil {
		t.Fatal(err)
	}
	for _, tc := range cases {
		t.Run(tc.Name, func(t *testing.T) {
			code, err := RealmInviteCode(tc.Input)
			if tc.Code == nil {
				if err == nil {
					t.Fatalf("unexpected code %q", code)
				}
			} else if err != nil || code != *tc.Code {
				t.Fatalf("code %q, error %v", code, err)
			}
		})
	}
	at := strings.Repeat("a", bridgecontract.RealmInviteInputBytes)
	if code, err := RealmInviteCode(at); err != nil || code != at {
		t.Fatal(code, err)
	}
	for _, input := range []string{at + "a", at + "/short"} {
		if _, err := RealmInviteCode(input); err == nil {
			t.Fatal("accepted an input over the original byte bound")
		}
	}
}

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
