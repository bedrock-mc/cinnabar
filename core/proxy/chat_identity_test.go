package proxy

import (
	"bytes"
	"context"
	"errors"
	"io"
	"log/slog"
	"testing"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestUpstreamLoginNameMatchesServerIdentity(t *testing.T) {
	for _, name := range []string{"Account Player", "ModernTag1234", "Guest", ""} {
		t.Run(name, func(t *testing.T) {
			network := newMemoryNetwork()
			listener, err := minecraft.ListenConfig{
				AuthenticationDisabled: true,
				ErrorLog:               slog.New(slog.DiscardHandler),
			}.ListenNetwork(network, "")
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { _ = listener.Close() })
			accepted := make(chan *minecraft.Conn, 1)
			go func() {
				conn, err := listener.Accept()
				if err == nil {
					server := conn.(*minecraft.Conn)
					accepted <- server
					_ = server.WritePacketImmediate(relayFixtureStartup()...)
				}
			}()
			dialer := newUpstreamDialer(dialerTestDownstream{
				identity: login.IdentityData{DisplayName: name},
				client:   login.ClientData{ThirdPartyName: "Transport Player"},
				protocol: minecraft.DefaultProtocol,
			}, nil)
			ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
			defer cancel()
			client, err := dialer.DialContextNetwork(ctx, network, "")
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { _ = client.Close() })
			select {
			case server := <-accepted:
				t.Cleanup(func() { _ = server.Close() })
				if client.IdentityData().DisplayName == "" {
					t.Fatal("the upstream login has no display name")
				}
				if got, want := client.IdentityData(), server.IdentityData(); got != want {
					t.Fatalf("client login identity = %#v, server parsed %#v", got, want)
				}
			case <-ctx.Done():
				t.Fatal(ctx.Err())
			}
		})
	}
}

func TestRelayChatUsesCurrentLoginIdentity(t *testing.T) {
	for _, identity := range []login.IdentityData{
		{DisplayName: "Account Player", XUID: "2535"},
		{DisplayName: "ModernTag1234", XUID: "2536"},
		{DisplayName: "Guest"},
	} {
		for _, variant := range []string{"direct", "transfer", "disconnect", "both", "delay"} {
			t.Run(identity.DisplayName+"/"+variant, func(t *testing.T) {
				down, up := newFakeDownstream(nil), newFakeUpstream(nil)
				t.Cleanup(func() { _ = down.Close(); _ = up.Close() })
				up.identity = identity
				var destination upstreamSession = up
				if variant == "transfer" || variant == "both" || variant == "delay" {
					destination = observeTransfers(destination, new(TransferState), nil)
				}
				if variant == "disconnect" || variant == "both" || variant == "delay" {
					destination = observeDisconnects(destination, func(DisconnectInfo) {})
				}
				original := &packet.Text{TextType: packet.TextTypeChat, SourceName: "Ranked Nickname", XUID: "old-account", Message: "hello"}
				before := encodeTestPacket(original)
				down.useBatchReads = true
				down.batchReads <- batchResult{packets: []packet.Packet{original}}
				down.batchReads <- batchResult{err: io.EOF}
				var delay *PacketDelay
				if variant == "delay" {
					delay = new(PacketDelay)
				}
				if err := pumpPacketsWithDelay(context.Background(), delay, down, destination, true); !errors.Is(err, io.EOF) {
					t.Fatal(err)
				}
				batches := up.flushedBatches()
				if len(batches) != 1 || len(batches[0]) != 1 {
					t.Fatalf("forwarded batches = %#v", batches)
				}
				got := batches[0][0].(*packet.Text)
				if got.SourceName != identity.DisplayName || got.XUID != identity.XUID || got.Message != original.Message {
					t.Fatalf("forwarded chat = %#v, want login identity %#v", got, identity)
				}
				if !bytes.Equal(before, encodeTestPacket(original)) {
					t.Fatal("the relay mutated the received chat packet")
				}
			})
		}
	}
}

func TestRelayPreservesServerChatPresentation(t *testing.T) {
	up, down := newFakeUpstream(nil), newFakeDownstream(nil)
	t.Cleanup(func() { _ = up.Close(); _ = down.Close() })
	original := &packet.Text{TextType: packet.TextTypeChat, SourceName: "Ranked Nickname", XUID: "2535", Message: "hello"}
	up.useBatchReads = true
	up.batchReads <- batchResult{packets: []packet.Packet{original}}
	up.batchReads <- batchResult{err: io.EOF}
	if err := pumpPackets(up, down, false); !errors.Is(err, io.EOF) {
		t.Fatal(err)
	}
	batches := down.flushedBatches()
	if len(batches) != 1 || len(batches[0]) != 1 || !bytes.Equal(encodeTestPacket(batches[0][0]), encodeTestPacket(original)) {
		t.Fatalf("server chat changed during relay: %#v", batches)
	}
}
