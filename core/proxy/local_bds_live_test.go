//go:build integration

package proxy

import (
	"context"
	"log/slog"
	"os"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/hashimthearab/rust-mcbe/core/localworld"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
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
