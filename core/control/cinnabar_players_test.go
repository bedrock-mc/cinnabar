package control

import (
	"encoding/json"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/cinnabarapi"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

type fakeSharedPlayers struct {
	fakeCinnabarAPI
	states  chan cinnabarapi.PlayerState
	updates cinnabarapi.PlayerUpdates
}

// SyncPlayers exposes one deterministic account snapshot to the real loopback control handler.
func (api *fakeSharedPlayers) SyncPlayers(state cinnabarapi.PlayerState) cinnabarapi.PlayerUpdates {
	api.states <- state
	return api.updates
}

func TestNearbyPlayerControlExchangesHatsAndClearsWithoutAnAccount(t *testing.T) {
	api := &fakeSharedPlayers{states: make(chan cinnabarapi.PlayerState, 1), updates: cinnabarapi.PlayerUpdates{
		Loadouts: []bridgecontract.CinnabarRemoteLoadout{{XUID: "11", UUID: "00112233-4455-6677-8899-aabbccddeeff", Revision: 1, Slots: []bridgecontract.CinnabarCosmeticSlot{{Slot: "head", ItemID: "cinnabar:bunny_ears"}}}},
	}}
	dir, _ := startCinnabarControl(t, api)
	request, _ := json.Marshal(map[string]any{"jsonrpc": "2.0", "id": 1, "method": bridgecontract.MethodCinnabarPlayers,
		"params": cinnabarapi.PlayerState{XUIDs: []string{"11"}, Slots: []bridgecontract.CinnabarCosmeticSlot{{Slot: "head", ItemID: "cinnabar:bunny_ears"}}}})
	payload := exchange(t, dir, request)
	if err := rpcError(t, payload); err != nil {
		t.Fatal(err)
	}
	state := <-api.states
	if len(state.Slots) != 1 || state.Slots[0].ItemID != "cinnabar:bunny_ears" || len(state.XUIDs) != 1 || state.XUIDs[0] != "11" {
		t.Fatal(state)
	}
	var answer struct {
		Result bridgecontract.CinnabarPlayerUpdates `json:"result"`
	}
	if err := json.Unmarshal(payload, &answer); err != nil {
		t.Fatal(err)
	}
	if len(answer.Result.Loadouts) != 1 || answer.Result.Loadouts[0].Slots[0].ItemID != "cinnabar:bunny_ears" || answer.Result.Emotes == nil || answer.Result.OwnedCosmetics == nil {
		t.Fatalf("updates: %s", payload)
	}
	offline, _ := startCinnabarControl(t, nil)
	payload = exchange(t, offline, request)
	if err := json.Unmarshal(payload, &answer); err != nil {
		t.Fatal(err)
	}
	if answer.Result.Loadouts == nil || len(answer.Result.Loadouts) != 0 || answer.Result.Emotes == nil || answer.Result.OwnedCosmetics == nil {
		t.Fatalf("offline updates: %s", payload)
	}
}

func TestNearbyPlayerControlForwardsPaidOwnership(t *testing.T) {
	api := &fakeSharedPlayers{states: make(chan cinnabarapi.PlayerState, 1), updates: cinnabarapi.PlayerUpdates{
		OwnershipXUID: "11", OwnedCosmetics: []string{"fixture:paid"},
	}}
	dir, _ := startCinnabarControl(t, api)
	request, _ := json.Marshal(map[string]any{"jsonrpc": "2.0", "id": 1, "method": bridgecontract.MethodCinnabarPlayers,
		"params": cinnabarapi.PlayerState{}})
	payload := exchange(t, dir, request)
	var answer struct {
		Result bridgecontract.CinnabarPlayerUpdates `json:"result"`
	}
	if err := json.Unmarshal(payload, &answer); err != nil {
		t.Fatal(err)
	}
	if answer.Result.OwnershipXUID != "11" || len(answer.Result.OwnedCosmetics) != 1 || answer.Result.OwnedCosmetics[0] != "fixture:paid" {
		t.Fatalf("ownership updates: %s", payload)
	}
}
