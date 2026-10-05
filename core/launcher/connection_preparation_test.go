package launcher

import (
	"context"
	"errors"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/control"
)

func TestPrepareConnectValidatesWithoutSelectingOrJoining(t *testing.T) {
	f := newFixture(t, testAccount())
	f.selector.Set("actual.example:1")
	for _, target := range []struct{ kind, value string }{
		{control.TargetRakNet, " selected.example:1 "},
		{control.TargetRealm, "42"},
		{control.TargetFriend, "123"},
		{control.TargetGathering, "5b0f2bd4-8a8e-4a6e-9d3c-0a1b2c3d4e5f"},
		{"", ""},
	} {
		if err := f.service.PrepareConnect(t.Context(), target.kind, target.value); err != nil {
			t.Fatal(err)
		}
	}
	if target, _ := f.selector.Target(); target != "actual.example:1" {
		t.Fatal("preparation changed the actual route")
	}
	for _, value := range []string{"host", "host:0", "host:65536", "bad host:1", ""} {
		if err := f.service.PrepareConnect(t.Context(), control.TargetRakNet, value); !errors.Is(err, control.ErrInvalidTarget) {
			t.Fatalf("preparation accepted malformed target %q", value)
		}
	}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if err := f.service.PrepareConnect(ctx, control.TargetRakNet, "selected:1"); !errors.Is(err, context.Canceled) {
		t.Fatal("canceled request prepared a transport")
	}
}

func TestPrepareConnectNeedsAccountOnlyForAccountTargetsAndRejectsLateSignout(t *testing.T) {
	f := newFixture(t, nil)
	if err := f.service.PrepareConnect(t.Context(), control.TargetRakNet, "selected:1"); err != nil {
		t.Fatal(err)
	}
	if err := f.service.PrepareConnect(t.Context(), control.TargetRealm, "42"); !errors.Is(err, control.ErrSignedOut) {
		t.Fatal("preparation accepted an account-bound target without an account")
	}
	f = newFixture(t, testAccount())
	if err := f.service.SignOut(); err != nil {
		t.Fatal(err)
	}
	if err := f.service.PrepareConnect(t.Context(), control.TargetRakNet, "late:1"); !errors.Is(err, control.ErrSignedOut) {
		t.Fatal("late preparation resumed network work after sign-out")
	}
	if err := f.service.PrepareConnect(t.Context(), "", ""); err != nil {
		t.Fatal("sign-out prevented preparation cancellation")
	}
}
