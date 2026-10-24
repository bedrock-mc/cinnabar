package proxy

import (
	"context"
	"net"
	"testing"
	"time"

	"github.com/df-mc/go-nethernet/discovery"
	"github.com/hashimthearab/rust-mcbe/core/localworld"
	"github.com/pion/webrtc/v4"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/p2p"
)

func TestLocalWorldTransportsGatherLoopbackCandidates(t *testing.T) {
	for _, transport := range []localworld.Transport{localworld.TransportNetherNetHTTP, localworld.TransportNetherNetLAN} {
		t.Run(string(transport), func(t *testing.T) {
			ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
			defer cancel()
			conn, err := net.ListenPacket("udp4", "127.0.0.1:0")
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { _ = conn.Close() })
			data := discovery.ServerData{LevelName: "selected", AcceptsSelfSignedAuth: true, Nonce: "fixture", ConnectionType: p2p.ConnectionTypeSignalingOverLAN}
			response, err := data.MarshalBinary()
			if err != nil {
				t.Fatal(err)
			}
			go func() {
				buf := make([]byte, 2048)
				for {
					n, addr, err := conn.ReadFrom(buf)
					if err != nil {
						return
					}
					packet, _, err := discovery.Unmarshal(buf[:n])
					if _, ok := packet.(*discovery.RequestPacket); err == nil && ok {
						_, _ = conn.WriteTo(discovery.Marshal(&discovery.ResponsePacket{ApplicationData: response}, 42), addr)
					}
				}
			}()
			resolve := withLocalTarget(func(context.Context) (localworld.ConnectionTarget, bool, error) {
				return localworld.ConnectionTarget{Transport: transport, Address: conn.LocalAddr().String(), LANAddress: conn.LocalAddr().String(), LevelName: data.LevelName}, true, nil
			}, onlineStub("must not resolve online signaling"))
			target, err := resolve(ctx)
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { _ = target.close() })
			var api *webrtc.API
			switch network := target.network.(type) {
			case localNetherNetNetwork:
				api = network.Dialer.API
			case minecraft.NetherNet:
				api = network.Dialer.API
			default:
				t.Fatalf("unexpected local transport %T", target.network)
			}
			if api == nil {
				api = webrtc.NewAPI()
			}
			gatherer, err := api.NewICEGatherer(webrtc.ICEGatherOptions{})
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { _ = gatherer.Close() })
			done := make(chan struct{})
			gatherer.OnLocalCandidate(func(candidate *webrtc.ICECandidate) {
				if candidate == nil {
					close(done)
				}
			})
			if err := gatherer.Gather(); err != nil {
				t.Fatal(err)
			}
			select {
			case <-done:
			case <-ctx.Done():
				t.Fatal(ctx.Err())
			}
			candidates, err := gatherer.GetLocalCandidates()
			if err != nil {
				t.Fatal(err)
			}
			for _, candidate := range candidates {
				if net.ParseIP(candidate.Address).IsLoopback() {
					return
				}
			}
			t.Fatal("local world has no loopback ICE candidate; it requires an external interface")
		})
	}
}
