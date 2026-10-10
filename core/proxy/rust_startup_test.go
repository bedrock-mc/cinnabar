package proxy

import (
	"context"
	"fmt"
	"io"
	"log/slog"
	"os"
	"reflect"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/device"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// TestProxyRustStartupHarness serves the session endpoint against an offline scripted upstream.
func TestProxyRustStartupHarness(t *testing.T) {
	scenario := os.Getenv("CINNABAR_STARTUP_FIXTURE")
	if scenario == "" {
		t.Skip("started by the Rust protocol integration test")
	}
	socketDir, err := externalRustClientSocketDir()
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	upstreamNetwork := newStreamNetwork()
	packs := rustStartupPacks(t, scenario)
	upstream, err := (minecraft.ListenConfig{
		AuthenticationDisabled: true, FlushRate: -1, ErrorLog: slog.New(slog.DiscardHandler),
		PrepareResourcePackOffer: func(_ context.Context, conn *minecraft.Conn) error {
			return conn.ConfigureResourcePackOffer(packs, packs != nil)
		},
	}).ListenNetwork(upstreamNetwork, "")
	if err != nil {
		t.Fatal(err)
	}
	defer upstream.Close()
	serverDone := make(chan error, 1)
	go func() {
		accepted, err := upstream.Accept()
		if err != nil {
			serverDone <- err
			return
		}
		conn := accepted.(*minecraft.Conn)
		err = runRustStartupScript(conn, scenario)
		_ = conn.Close()
		serverDone <- err
	}()
	connections := newPreparedConnections("unused.invalid:19132", nil, slog.New(slog.DiscardHandler))
	connections.resolveTarget = func(context.Context) (*resolvedUpstreamTarget, error) {
		return &resolvedUpstreamTarget{network: upstreamNetwork}, nil
	}
	connections.dialTarget = func(ctx context.Context, target *resolvedUpstreamTarget, dialer minecraft.Dialer) (upstreamSession, error) {
		return dialer.DialContextNetwork(ctx, target.network, "")
	}
	listener, err := streamnet.ListenSession(socketDir)
	if err != nil {
		t.Fatal(err)
	}
	claimed := device.New(protocol.DeviceAndroid)
	server := &sessionServer{listener: listener, prepared: connections, transfers: new(TransferState), logger: slog.New(slog.DiscardHandler), device: &claimed}
	server.start(ctx)
	defer connections.finishShutdown()
	defer server.close()
	defer connections.beginShutdown()
	fmt.Printf("RUST_MCBE_EXTERNAL_READY=%s\n", socketDir)
	if _, err := io.Copy(io.Discard, os.Stdin); err != nil {
		t.Fatal(err)
	}
	select {
	case err := <-serverDone:
		if err != nil {
			t.Fatal(err)
		}
	case <-ctx.Done():
		t.Fatal(ctx.Err())
	}
}

// rustStartupPacks is the required offer of the packs scenario: an encrypted pack, then a plain one.
func rustStartupPacks(t *testing.T, scenario string) []*resource.Pack {
	if scenario != "packs" {
		return nil
	}
	var packs []*resource.Pack
	for _, id := range []string{"00112233-4455-6677-8899-aabbccddeeff", "11223344-5566-7788-99aa-bbccddeeff00"} {
		pack, err := resource.ReadBytes(admissionPackArchiveWithID(t, id))
		if err != nil {
			t.Fatal(err)
		}
		packs = append(packs, pack)
	}
	packs[0] = packs[0].WithContentKey("fixture-content-key")
	return packs
}

// runRustStartupScript asserts each client message at the upstream end of the session.
func runRustStartupScript(conn *minecraft.Conn, scenario string) error {
	startup := relayFixtureStartup()
	transfer := &packet.Transfer{Address: "next.example.test", Port: 19133}
	switch scenario {
	case "transfer-batch":
		return conn.WritePacketImmediate(startup[0], transfer)
	case "refused":
		return conn.WritePacketImmediate(&packet.Disconnect{Message: "fixture server is full"})
	}
	if err := conn.WritePacketImmediate(startup[0]); err != nil {
		return err
	}
	switch scenario {
	case "transfer":
		return conn.WritePacketImmediate(transfer)
	case "disconnect":
		return conn.WritePacketImmediate(&packet.Disconnect{Message: "fixture kicked during spawn"})
	}
	for _, expected := range []packet.Packet{&packet.RequestChunkRadius{ChunkRadius: 16, MaxChunkRadius: 16}, &packet.ServerBoundLoadingScreen{Type: packet.LoadingScreenTypeStart}} {
		if err := expectStartupPacket(conn, expected); err != nil {
			return err
		}
	}
	// Readiness prerequisites arrive only after both initial client messages.
	if err := conn.WritePacketImmediate(startup[1:]...); err != nil {
		return err
	}
	if err := expectStartupPacket(conn, &packet.NetworkStackLatency{Timestamp: 100}); err != nil {
		return fmt.Errorf("initialized before presentation readiness: %w", err)
	}
	if err := conn.WritePacketImmediate(&packet.SetTime{Time: 200}); err != nil {
		return err
	}
	for _, expected := range []packet.Packet{&packet.ServerBoundLoadingScreen{Type: packet.LoadingScreenTypeEnd}, &packet.SetLocalPlayerAsInitialised{EntityRuntimeID: 42}, &packet.NetworkStackLatency{Timestamp: 300}} {
		if err := expectStartupPacket(conn, expected); err != nil {
			return err
		}
	}
	if err := conn.WritePacketImmediate(&packet.SetTime{Time: 400}); err != nil || scenario != "transfer-play" {
		return err
	}
	return conn.WritePacketImmediate(&packet.Transfer{Address: "play.example.test", Port: 19134})
}

// expectStartupPacket compares complete decoded packets, including order and runtime identity.
func expectStartupPacket(conn *minecraft.Conn, expected packet.Packet) error {
	actual, err := conn.ReadPacket()
	if err != nil {
		return err
	}
	if !reflect.DeepEqual(actual, expected) {
		return fmt.Errorf("received %#v, want %#v", actual, expected)
	}
	return nil
}
