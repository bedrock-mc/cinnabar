package proxy

import (
	"context"
	"sync/atomic"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
	"github.com/hashimthearab/rust-mcbe/core/localworld"
	"github.com/pion/webrtc/v4"
)

// A local world must offer loopback candidates even when Wi-Fi and mobile data are disabled.
func TestLocalBDSTransportGathersLoopbackCandidates(t *testing.T) {
	resolve := withLocalTarget(func(context.Context) (localworld.ConnectionTarget, bool, error) {
		return localworld.ConnectionTarget{Address: "127.0.0.1:5000", Transport: localworld.TransportNetherNetHTTP}, true, nil
	}, onlineStub("unused"))
	target, err := resolve(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	network := target.network.(localNetherNetNetwork)
	api := network.Dialer.API
	if api == nil {
		api = webrtc.NewAPI()
	}
	peer, err := api.NewPeerConnection(webrtc.Configuration{})
	if err != nil {
		t.Fatal(err)
	}
	defer peer.Close()
	var loopback atomic.Bool
	peer.OnICECandidate(func(candidate *webrtc.ICECandidate) {
		if candidate != nil && candidate.Address == "127.0.0.1" && candidate.Typ == webrtc.ICECandidateTypeHost {
			loopback.Store(true)
		}
	})
	if _, err := peer.CreateDataChannel("local-world", nil); err != nil {
		t.Fatal(err)
	}
	complete := webrtc.GatheringCompletePromise(peer)
	offer, err := peer.CreateOffer(nil)
	if err != nil {
		t.Fatal(err)
	}
	if err := peer.SetLocalDescription(offer); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(t.Context(), testwait.DefaultTimeout)
	defer cancel()
	select {
	case <-complete:
	case <-ctx.Done():
		t.Fatal("local ICE gathering did not complete")
	}
	if !loopback.Load() {
		t.Fatal("local BDS offer has no IPv4 loopback host candidate")
	}
}
