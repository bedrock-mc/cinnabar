//go:build integration

package proxy

import (
	"context"
	"log/slog"
	"os"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/localworld"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"golang.org/x/oauth2"
)

// This opt-in probe opens and closes only the transport: it never logs in,
// requests a world or changes the configured server's persistent state.
func TestLocalBDSLiveOfflineTransport(t *testing.T) {
	address := os.Getenv("CINNABAR_TEST_BDS_ADDRESS")
	if address == "" {
		t.Skip("set CINNABAR_TEST_BDS_ADDRESS to an explicitly approved loopback BDS")
	}
	ctx, cancel := context.WithTimeout(t.Context(), 12*time.Second)
	defer cancel()
	resolve := withLocalTarget(func(context.Context) (localworld.ConnectionTarget, bool, error) {
		return localworld.ConnectionTarget{Address: address, Transport: localworld.TransportNetherNetHTTP}, true, nil
	}, onlineStub("must not use online signaling"))
	target, err := resolve(ctx)
	if err != nil {
		t.Fatal(err)
	}
	conn, err := target.network.DialContext(ctx, target.address)
	if err != nil {
		t.Fatal(err)
	}
	if err := conn.Close(); err != nil {
		t.Fatal(err)
	}
	t.Log("production offline HTTP SDP/ICE transport connected and closed without Minecraft login")
}

// Injects only the keyless rejection, then uses the production retry to enter a real world.
func TestLocalBDSLiveKeylessLoginReachesWorld(t *testing.T) {
	address := os.Getenv("CINNABAR_TEST_BDS_ADDRESS")
	if address == "" {
		t.Skip("set CINNABAR_TEST_BDS_ADDRESS to an explicitly approved loopback BDS")
	}
	ctx, cancel := context.WithTimeout(t.Context(), 30*time.Second)
	defer cancel()
	account := authcache.NewAccount(ctx, "", oauth2.StaticTokenSource(&oauth2.Token{AccessToken: "unused"}), nil)
	defer account.Close()
	connections := newPreparedConnections("unused.invalid:19132", account, slog.New(slog.DiscardHandler))
	defer connections.shutdown()
	connections.resolveTarget = withLocalTarget(func(context.Context) (localworld.ConnectionTarget, bool, error) {
		return localworld.ConnectionTarget{Address: address, Transport: localworld.TransportNetherNetHTTP}, true, nil
	}, onlineStub("must not use online signaling"))
	dial := connections.dialTarget
	attempts := 0
	connections.dialTarget = func(ctx context.Context, target *resolvedUpstreamTarget, dialer minecraft.Dialer) (upstreamSession, error) {
		attempts++
		if attempts == 1 {
			if dialer.TokenSource != account {
				t.Fatal("initial login lost the signed-in account")
			}
			return nil, &localBDSLoginError{err: &minecraft.DisconnectPacketError{Reason: packet.DisconnectReasonHostWithoutKeys}}
		}
		return dial(ctx, target, dialer)
	}
	prepared, err := connections.connect(ctx, dialerTestDownstream{
		protocol: minecraft.DefaultProtocol,
		identity: login.IdentityData{DisplayName: "LocalPlayer", Identity: uuid.NewString()},
	})
	if err != nil {
		t.Fatalf("account-available local login: %v", err)
	}
	defer prepared.close()
	if attempts != 2 {
		t.Fatalf("login attempts = %d, want 2", attempts)
	}
	conn := prepared.upstream.(*minecraft.Conn)
	deadline, _ := ctx.Deadline()
	if err := conn.SetReadDeadline(deadline); err != nil {
		t.Fatal(err)
	}
	for {
		batch, err := conn.ReadBatch()
		if err != nil {
			t.Fatalf("read world handoff: %v", err)
		}
		for _, pk := range batch {
			if start, ok := pk.(*packet.StartGame); ok {
				if start.EntityRuntimeID == 0 {
					t.Fatal("world handoff has no player")
				}
				return
			}
		}
	}
}

// This opt-in probe logs in signed out and spawns, as the friend host's relay into BDS does.
func TestLocalBDSLiveSignedOutLoginSpawns(t *testing.T) {
	address := os.Getenv("CINNABAR_TEST_BDS_ADDRESS")
	if address == "" {
		t.Skip("set CINNABAR_TEST_BDS_ADDRESS to an explicitly approved loopback BDS")
	}
	ctx, cancel := context.WithTimeout(t.Context(), 30*time.Second)
	defer cancel()
	resolve := withLocalTarget(func(context.Context) (localworld.ConnectionTarget, bool, error) {
		return localworld.ConnectionTarget{Address: address, Transport: localworld.TransportNetherNetHTTP}, true, nil
	}, onlineStub("must not use online signaling"))
	target, err := resolve(ctx)
	if err != nil {
		t.Fatal(err)
	}
	conn, err := minecraft.Dialer{
		IdentityData: login.IdentityData{DisplayName: "SignedOut", Identity: uuid.NewString()},
		ErrorLog:     slog.New(slog.DiscardHandler),
	}.DialContextNetwork(ctx, target.network, target.address)
	if err != nil {
		t.Fatalf("signed-out login: %v", err)
	}
	defer conn.Close()
	if err := conn.DoSpawnContext(ctx); err != nil {
		t.Fatalf("spawn: %v", err)
	}
}
