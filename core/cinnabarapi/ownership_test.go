package cinnabarapi

import (
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
)

func TestOwnershipUsesAuthenticatedSessionAndWithdrawsRevokedGrants(t *testing.T) {
	api := newFakeAPI(t)
	api.ownedCosmetics = []string{"fixture:paid"}
	client := newTestClient(t, api, nil)
	client.Configure(testConfig)
	session := receive(t, api.sessions)
	testwait.Eventually(t, testwait.DefaultTimeout, "welcome ownership", func() bool {
		got := client.SyncPlayers(PlayerState{})
		return got.OwnershipXUID == "2535400000000001" && len(got.OwnedCosmetics) == 1 && got.OwnedCosmetics[0] == "fixture:paid"
	})
	session.send(t, map[string]any{"type": typeCosmeticOwnership, "owned_cosmetics": []string{}})
	testwait.Eventually(t, testwait.DefaultTimeout, "revoked ownership", func() bool {
		return len(client.SyncPlayers(PlayerState{}).OwnedCosmetics) == 0
	})
	client.mu.Lock()
	client.live = nil
	client.mu.Unlock()
	got := client.SyncPlayers(PlayerState{})
	if got.OwnershipXUID != "" || len(got.OwnedCosmetics) != 0 {
		t.Fatal("disconnected account retained grants", got)
	}
}

func TestMalformedOwnershipFailsClosed(t *testing.T) {
	if got := admittedOwnership([]string{"fixture:capes/paid"}); len(got) != 1 {
		t.Fatal("schema-valid nested path was rejected", got)
	}
	if got := admittedOwnership([]string{"fixture:paid", "invalid/item"}); got != nil {
		t.Fatal(got)
	}
	if got := admittedOwnership([]string{"fixture:paid", "fixture:paid"}); len(got) != 1 {
		t.Fatal(got)
	}
}
