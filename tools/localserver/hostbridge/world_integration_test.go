//go:build integration

package hostbridge

import (
	"bytes"
	"context"
	"encoding/hex"
	"encoding/json"
	"log/slog"
	"net"
	"sync"
	"testing"
	"time"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/internal/hostcontract"
)

// TestBridgeActualDragonflyWorldEntry uses the real server finalizer, player/world and packet output.
// It qualifies the Go integration only, not a Rust handshake or service-backed player approval.
func TestBridgeActualDragonflyWorldEntry(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	defer cancel()
	parent, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer parent.Close()
	l, err := New(hostcontract.MAX_PLAYERS)
	if err != nil {
		t.Fatal(err)
	}
	configuration := server.Config{Name: "fixture-world", MaxPlayers: hostcontract.MAX_PLAYERS, MaxChunkRadius: 1,
		Log: slog.New(slog.DiscardHandler), Listeners: []func(server.Config) (server.Listener, error){l.Factory},
		Generator: func(world.Dimension) world.Generator { return world.NopGenerator{} }}
	srv := configuration.New()
	srv.Listen()
	accepted := make(chan bool, 1)
	acceptDone := make(chan struct{})
	expectedIdentity := mustUUID(t, "00000000-0000-4000-8000-000000000001")
	go func() {
		defer close(acceptDone)
		for p := range srv.Accept() {
			accepted <- p.Name() == "Fixture" && p.UUID() == expectedIdentity
			p.Message("native world fixture")
		}
	}()
	defer func() { _ = l.Close(); _ = srv.Close(); <-acceptDone }()
	bootstrap := hostcontract.Bootstrap{Address: parent.Addr().String(),
		Opening:    hex.EncodeToString(bytes.Repeat([]byte{8}, hostcontract.OPENING_BYTES)),
		Capability: hex.EncodeToString(bytes.Repeat([]byte{7}, hostcontract.CAPABILITY_BYTES))}
	if err := l.Start(ctx, bootstrap); err != nil {
		t.Fatal(err)
	}
	control := fixtureAccept(t, parent, bootstrap, 0)
	defer control.Close()
	var snapshot hostcontract.Snapshot
	fixtureReadJSON(t, control, hostcontract.KindSnapshot, &snapshot)
	data, err := json.Marshal(login.ClientData{DeviceOS: protocol.DeviceAndroid, GameVersion: protocol.CurrentVersion,
		LanguageCode: "en_GB", SkinID: "fixture", ThirdPartyName: "Fixture"})
	if err != nil {
		t.Fatal(err)
	}
	if err := control.json(hostcontract.KindOpen, hostcontract.Open{Peer: 1, RemainingMillis: 4000,
		Identity: hostcontract.Identity{Xuid: "1234", Uuid: "00000000-0000-4000-8000-000000000001", DisplayName: "Fixture"}, ClientData: data}); err != nil {
		t.Fatal(err)
	}
	peer := fixtureAccept(t, parent, bootstrap, 1)
	defer peer.Close()
	var opened hostcontract.PeerResult
	fixtureReadJSON(t, control, hostcontract.KindOpened, &opened)
	if err := peer.write(hostcontract.KindAttach, nil); err != nil {
		t.Fatal(err)
	}
	kind, body, err := peer.read()
	if err != nil || kind != hostcontract.KindWorld {
		t.Fatal("real server finalization did not produce world output")
	}
	fixtureWorld(t, body, 1)
	if err := peer.json(hostcontract.KindInitialized, hostcontract.Initialized{Radius: 1}); err != nil {
		t.Fatal(err)
	}
	message := make(chan struct{})
	closed := make(chan error, 1)
	readDone := make(chan struct{})
	go func() {
		defer close(readDone)
		var seen sync.Once
		var payload bool
		for {
			kind, body, err := peer.read()
			if err != nil {
				closed <- err
				return
			}
			switch kind {
			case hostcontract.KindPacket:
				if payload {
					closed <- errProtocol
					return
				}
				payload = true
				var header packet.Header
				if header.Read(bytes.NewBuffer(body)) != nil {
					closed <- errProtocol
					return
				}
				if header.PacketID == packet.IDText {
					seen.Do(func() { close(message) })
				}
			case hostcontract.KindFlush:
				if !payload {
					closed <- errProtocol
					return
				}
				payload = false
				if err := peer.write(hostcontract.KindFlushed, body); err != nil {
					closed <- err
					return
				}
			case hostcontract.KindClosed:
				if payload {
					closed <- errProtocol
					return
				}
				closed <- nil
				return
			default:
				closed <- errProtocol
				return
			}
		}
	}()
	defer func() { _ = peer.Close(); <-readDone }()
	select {
	case valid := <-accepted:
		if !valid {
			t.Fatal("real world lost supplied player presentation")
		}
	case err := <-closed:
		t.Fatalf("world peer ended before entry: %v", err)
	case <-ctx.Done():
		t.Fatal("real world entry did not finish")
	}
	select {
	case <-message:
	case err := <-closed:
		t.Fatalf("world output ended: %v", err)
	case <-ctx.Done():
		t.Fatal("real world play packet did not arrive")
	}
	if err := peer.write(hostcontract.KindClose, nil); err != nil {
		t.Fatal(err)
	}
	if err := <-closed; err != nil {
		t.Fatal(err)
	}
}
