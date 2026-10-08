//go:build integration

package proxy

import (
	"context"
	"net"
	"os"
	"strconv"
	"testing"
	"time"

	"github.com/df-mc/go-nethernet"
	"github.com/df-mc/go-nethernet/discovery"
	"github.com/hashimthearab/rust-mcbe/core/localworld"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
)

// This explicit, loopback-only probe tests BDS's own advertised offline LAN
// policy. It stops before Minecraft Login, so it does not enter or mutate a world.
func TestLocalBDSLiveOfflineLANTransport(t *testing.T) {
	address := os.Getenv("CINNABAR_TEST_BDS_LAN_ADDRESS")
	if address == "" {
		t.Skip("set CINNABAR_TEST_BDS_LAN_ADDRESS to the approved loopback discovery mapping")
	}
	remote, err := net.ResolveUDPAddr("udp4", address)
	if err != nil || remote.IP == nil || !remote.IP.IsLoopback() || remote.Port == 0 {
		t.Fatalf("invalid loopback LAN target %q: %v", address, err)
	}
	signaling, err := (discovery.ListenConfig{BroadcastAddress: remote}).Listen(net.JoinHostPort(remote.IP.String(), "0"))
	if err != nil {
		t.Fatal(err)
	}
	defer signaling.Close()
	ctx, cancel := context.WithTimeout(t.Context(), 12*time.Second)
	defer cancel()
	ticker := time.NewTicker(50 * time.Millisecond)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			t.Fatal("no approved local BDS LAN response: ", ctx.Err())
		case <-ticker.C:
			for id, response := range signaling.Responses() {
				var data discovery.ServerData
				if err := data.UnmarshalBinary(response); err != nil {
					t.Fatal(err)
				}
				if expected := os.Getenv("CINNABAR_TEST_BDS_LEVEL"); expected != "" && data.LevelName != expected {
					continue
				}
				t.Logf("BDS LAN advertisement: online=%v offline=%v nonce_present=%v connection_type=%d players=%d/%d", data.AcceptsOnlineAuth, data.AcceptsSelfSignedAuth, data.Nonce != "", data.ConnectionType, data.PlayerCount, data.MaxPlayerCount)
				if !data.AcceptsSelfSignedAuth {
					t.Fatal("BDS does not advertise offline LAN admission; will not attempt to override its policy")
				}
				// Vanilla BDS's explicitly advertised offline LAN answer has no
				// identity assertion. This is not enabled for HTTP signaling.
				network := minecraft.NetherNet{Signaling: signaling, Dialer: nethernet.Dialer{AllowIdentitylessServer: true}}
				conn, err := network.DialContext(ctx, strconv.FormatUint(id, 10))
				if err != nil {
					t.Fatal(err)
				}
				if err := conn.Close(); err != nil {
					t.Fatal(err)
				}
				t.Log("offline LAN SDP/ICE connected and closed without Minecraft login")
				return
			}
		}
	}
}

func TestLocalBDSLiveProductionOfflineLANTransport(t *testing.T) {
	address, level := os.Getenv("CINNABAR_TEST_BDS_LAN_ADDRESS"), os.Getenv("CINNABAR_TEST_BDS_LEVEL")
	if address == "" || level == "" {
		t.Skip("set the explicitly approved loopback discovery mapping and selected BDS level")
	}
	ctx, cancel := context.WithTimeout(t.Context(), 12*time.Second)
	defer cancel()
	resolve := withLocalTarget(func(context.Context) (localworld.ConnectionTarget, bool, error) {
		return localworld.ConnectionTarget{Transport: localworld.TransportNetherNetLAN, LANAddress: address, LevelName: level}, true, nil
	}, onlineStub("must not resolve online signaling"))
	target, err := resolve(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer target.close()
	var client login.ClientData
	target.clientData(&client)
	if !target.offline || client.Nonce == "" {
		t.Fatal("production offline target must carry the advertised nonce")
	}
	conn, err := target.network.DialContext(ctx, target.address)
	if err != nil {
		t.Fatal(err)
	}
	if err := conn.Close(); err != nil {
		t.Fatal(err)
	}
	if err := target.close(); err != nil {
		t.Fatal(err)
	}
	network, ok := target.network.(minecraft.NetherNet)
	if !ok {
		t.Fatal("production LAN target must retain the NetherNet adapter")
	}
	select {
	case <-network.Signaling.Context().Done():
	default:
		t.Fatal("closing the production target leaked its discovery listener")
	}
	t.Log("production selected-level offline LAN transport connected; advertised nonce bound; no Xbox or Minecraft login")
}
