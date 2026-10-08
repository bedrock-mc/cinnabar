package proxy

import (
	"context"
	"errors"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
)

func TestLocalHostGrantUsesCanonicalIdentityOnlyForManagedWorld(t *testing.T) {
	for _, managed := range []bool{false, true} {
		upstream := &fakeUpstream{identity: login.IdentityData{DisplayName: "Canonical Host"}}
		calls := 0
		dial := grantLocalHostOnDial(func(context.Context, *resolvedUpstreamTarget, minecraft.Dialer) (upstreamSession, error) {
			return upstream, nil
		}, func(_ context.Context, address, name string) error {
			calls++
			if address != "127.0.0.1:1" || name != "Canonical Host" {
				t.Fatalf("grant = %q, %q", address, name)
			}
			return nil
		})
		target := &resolvedUpstreamTarget{}
		if managed {
			target.managedWorldAddress = "127.0.0.1:1"
		}
		got, err := dial(t.Context(), target, minecraft.Dialer{IdentityData: login.IdentityData{DisplayName: "Downstream Name"}})
		if err != nil || got != upstream || (calls == 1) != managed {
			t.Fatalf("upstream = %v, %v; calls = %d", got, err, calls)
		}
	}
}

func TestLocalHostGrantSkipsFailedJoinAndPropagatesFailure(t *testing.T) {
	boom := errors.New("fixture")
	calls := 0
	upstream := &fakeUpstream{identity: login.IdentityData{DisplayName: "Host"}}
	for _, dialError := range []error{boom, nil} {
		dial := grantLocalHostOnDial(func(context.Context, *resolvedUpstreamTarget, minecraft.Dialer) (upstreamSession, error) {
			return upstream, dialError
		}, func(context.Context, string, string) error { calls++; return boom })
		got, err := dial(t.Context(), &resolvedUpstreamTarget{managedWorldAddress: "127.0.0.1:1"}, minecraft.Dialer{})
		if !errors.Is(err, boom) || got != upstream {
			t.Fatal("failed grant lost owned upstream or error")
		}
	}
	if calls != 1 {
		t.Fatalf("grants = %d", calls)
	}
	// The friend admission path has no host callback, even for the same managed target.
	dial := grantLocalHostOnDial(func(context.Context, *resolvedUpstreamTarget, minecraft.Dialer) (upstreamSession, error) {
		return upstream, nil
	}, nil)
	if _, err := dial(t.Context(), &resolvedUpstreamTarget{managedWorldAddress: "127.0.0.1:1"}, minecraft.Dialer{}); err != nil {
		t.Fatal(err)
	}
}
