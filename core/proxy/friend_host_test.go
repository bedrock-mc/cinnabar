package proxy

import (
	"encoding/json"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft/p2p"
	"github.com/sandertv/gophertunnel/minecraft/room"
)

func TestReconcileNoncesIssuesOnePerMemberAndForgetsLeavers(t *testing.T) {
	nonces := map[string]string{}
	if !reconcileNonces(nonces, []string{"host", "a", "b", ""}, "host") {
		t.Fatal("new members did not change nonces")
	}
	if _, ok := nonces["host"]; ok || len(nonces) != 2 || nonces["a"] == "" || nonces["a"] == nonces["b"] {
		t.Fatalf("nonces = %v", nonces)
	}
	first := nonces["a"]
	if reconcileNonces(nonces, []string{"host", "a", "b"}, "host") {
		t.Fatal("unchanged membership rewrote nonces")
	}
	if nonces["a"] != first {
		t.Fatal("a staying member's nonce changed")
	}
	if !reconcileNonces(nonces, []string{"host", "b"}, "host") {
		t.Fatal("a leaver did not change nonces")
	}
	if _, ok := nonces["a"]; ok {
		t.Fatal("a leaver's nonce stayed replayable")
	}
	reconcileNonces(nonces, []string{"host", "a", "b"}, "host")
	if nonces["a"] == first {
		t.Fatal("a returning member reused its old nonce")
	}
}

func TestAdmitFriendNeedsTheIssuedNonceAndRefusesTheHost(t *testing.T) {
	for _, test := range []struct {
		name            string
		xuid, presented string
		issued          string
		ok, want        bool
	}{
		{"issued nonce", "2", "n", "n", true, true},
		{"wrong nonce", "2", "x", "n", true, false},
		{"no nonce issued", "2", "n", "", false, false},
		{"empty presented", "2", "", "", true, false},
		{"host account", "1", "n", "n", true, false},
		{"no xuid", "", "n", "n", true, false},
	} {
		if _, got := admitFriend("1", test.xuid, test.presented, test.issued, test.ok); got != test.want {
			t.Errorf("%s: admitted = %v, want %v", test.name, got, test.want)
		}
	}
}

// Vanilla clients decode the world card and wait for their nonce under "nonces".
func TestFriendSessionPropertiesDecodeAsAVanillaWorld(t *testing.T) {
	properties := friendSessionProperties{
		Status: room.Status{
			HostName: "Host", OwnerID: "1", WorldName: "My World", MemberCount: 1,
			BroadcastSetting: p2p.BroadcastSettingFriendsOfFriends, TransportLayer: p2p.TransportLayerNetherNet,
			SupportedConnections: []p2p.Connection{{Type: p2p.ConnectionTypeSignalingOverJSONRPC, NetherNetID: "42"}},
		},
		Nonces: map[string]string{"2": "n"},
	}
	encoded, err := json.Marshal(properties)
	if err != nil {
		t.Fatal(err)
	}
	var world p2p.World
	if err := json.Unmarshal(encoded, &world); err != nil {
		t.Fatal(err)
	}
	if world.WorldName != "My World" || world.OwnerID != "1" || world.Nonces["2"] != "n" {
		t.Fatalf("decoded world = %+v", world)
	}
	if !world.Listed("2", false, func(string) bool { return false }) {
		t.Fatal("friends of friends cannot see the world")
	}
}
