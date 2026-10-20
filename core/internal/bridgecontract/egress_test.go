package bridgecontract

import (
	_ "embed"
	"encoding/json"
	"net/netip"
	"testing"
)

//go:embed testdata/gateway-egress.json
var gatewayEgressCorpus []byte

func TestGatewayEgressSharedAddressBoundaries(t *testing.T) {
	var corpus struct {
		Schema int `json:"schema"`
		Cases  []struct {
			Address string `json:"address"`
			Public  bool   `json:"public"`
		} `json:"cases"`
	}
	if err := json.Unmarshal(gatewayEgressCorpus, &corpus); err != nil {
		t.Fatal(err)
	}
	if corpus.Schema != 1 || len(corpus.Cases) == 0 {
		t.Fatal("invalid gateway policy corpus")
	}
	for _, item := range corpus.Cases {
		address, err := netip.ParseAddr(item.Address)
		if err != nil {
			t.Fatal(err)
		}
		if got := PublicGatewayUpstream(address); got != item.Public {
			t.Errorf("gateway policy differs for %s: got %t, want %t", item.Address, got, item.Public)
		}
	}
	if PublicGatewayUpstream(netip.Addr{}) {
		t.Fatal("invalid Go address admitted")
	}
	// Preserve the existing Go zone semantics; the Rust typed-IP corpus has no zones.
	for _, text := range []string{"::%fixture", "2001:db8::1%fixture"} {
		if !PublicGatewayUpstream(netip.MustParseAddr(text)) {
			t.Fatalf("existing Go zoned-address behavior changed for %s", text)
		}
	}
	if PublicGatewayUpstream(netip.MustParseAddr("::1%fixture")) {
		t.Fatal("zoned loopback admitted")
	}
}
