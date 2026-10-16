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
		Hats: []bridgecontract.CinnabarRemoteHat{{XUID: "11", UUID: "00112233-4455-6677-8899-aabbccddeeff", HatID: "bunny_ears", ValidForMS: 15000}},
	}}
	dir, _ := startCinnabarControl(t, api)
	request, _ := json.Marshal(map[string]any{"jsonrpc": "2.0", "id": 1, "method": bridgecontract.MethodCinnabarPlayers,
		"params": cinnabarapi.PlayerState{XUIDs: []string{"11"}, HatID: "crown"}})
	payload := exchange(t, dir, request)
	if err := rpcError(t, payload); err != nil {
		t.Fatal(err)
	}
	state := <-api.states
	if state.HatID != "crown" || len(state.XUIDs) != 1 || state.XUIDs[0] != "11" {
		t.Fatal(state)
	}
	var answer struct {
		Result bridgecontract.CinnabarPlayerUpdates `json:"result"`
	}
	if err := json.Unmarshal(payload, &answer); err != nil {
		t.Fatal(err)
	}
	if len(answer.Result.Hats) != 1 || answer.Result.Hats[0].HatID != "bunny_ears" || answer.Result.Emotes == nil {
		t.Fatalf("updates: %s", payload)
	}
	offline, _ := startCinnabarControl(t, nil)
	payload = exchange(t, offline, request)
	if err := json.Unmarshal(payload, &answer); err != nil {
		t.Fatal(err)
	}
	if answer.Result.Hats == nil || len(answer.Result.Hats) != 0 || answer.Result.Emotes == nil {
		t.Fatalf("offline updates: %s", payload)
	}
}
