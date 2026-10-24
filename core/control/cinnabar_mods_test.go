package control

import (
	"encoding/json"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"testing"
)

type fakeMarketplace struct {
	fakeCinnabarAPI
	library bridgecontract.CinnabarModLibrary
}

// MarketplaceLibrary supplies one authenticated fixture to the real control handler.
func (api *fakeMarketplace) MarketplaceLibrary() bridgecontract.CinnabarModLibrary {
	return api.library
}

func TestMarketplaceControlReturnsOnlyCurrentAccountHints(t *testing.T) {
	api := &fakeMarketplace{library: bridgecontract.CinnabarModLibrary{XUID: "11", Mods: []bridgecontract.CinnabarModEntry{{Slug: "fixture-mod"}}}}
	dir, _ := startCinnabarControl(t, api)
	request := []byte(`{"jsonrpc":"2.0","id":1,"method":"cinnabar_mods.v1"}`)
	var answer struct {
		Result bridgecontract.CinnabarModLibrary `json:"result"`
	}
	if err := json.Unmarshal(exchange(t, dir, request), &answer); err != nil {
		t.Fatal(err)
	}
	if answer.Result.XUID != "11" || len(answer.Result.Mods) != 1 {
		t.Fatal(answer)
	}
	offline, _ := startCinnabarControl(t, nil)
	if err := json.Unmarshal(exchange(t, offline, request), &answer); err != nil {
		t.Fatal(err)
	}
	if answer.Result.XUID != "" || answer.Result.Mods == nil || len(answer.Result.Mods) != 0 {
		t.Fatal(answer)
	}
}
