package main

import (
	"bytes"
	"encoding/json"
	"image/png"
	"testing"

	"github.com/go-gl/mathgl/mgl32"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestTotemFixtureKeepsEquipActivationConsumptionInOrder(t *testing.T) {
	t.Parallel()
	for _, mode := range []string{"main", "offhand"} {
		t.Run(mode, func(t *testing.T) {
			packets := totemFixturePackets(mode, 42, mgl32.Vec3{2, 65, 3})
			activation := -1
			for index, pk := range packets {
				if event, ok := pk.(*packet.ActorEvent); ok {
					if event.EntityRuntimeID != 42 || event.EventType != packet.ActorEventTalismanActivate {
						t.Fatal("activation addressed another actor")
					}
					activation = index
				}
			}
			if activation < 2 {
				t.Fatal("activation preceded equipped item")
			}
			if mode == "main" {
				selection := -1
				for index, pk := range packets[:activation] {
					if hotbar, ok := pk.(*packet.PlayerHotBar); ok && hotbar.SelectHotBarSlot && hotbar.SelectedHotBarSlot == 0 {
						selection = index
					}
				}
				if selection < 0 {
					t.Fatal("main-hand activation must authoritatively select its equipped slot")
				}
			}
			equipped := packets[activation-1].(*packet.MobEquipment)
			if equipped.NewItem.Stack.Count != 1 || equipped.NewItem.Stack.NetworkID == 0 {
				t.Fatal("activation has no negotiated totem")
			}
			consumed := packets[len(packets)-1].(*packet.MobEquipment)
			if consumed.NewItem.Stack.Count != 0 || consumed.WindowID != equipped.WindowID {
				t.Fatal("consumption did not clear the equipped hand")
			}
		})
	}
}

func TestTotemFixtureStaleEchoSendsOnlyAnEmptyLocalOffhandEcho(t *testing.T) {
	t.Parallel()
	packets := totemFixturePackets("staleecho", 42, mgl32.Vec3{})
	if len(packets) != 1 {
		t.Fatalf("got %d packets, want only the offhand echo", len(packets))
	}
	echo := packets[0].(*packet.MobEquipment)
	if echo.EntityRuntimeID != 42 || echo.WindowID != 119 || echo.NewItem.Stack.Count != 0 {
		t.Fatal("echo is not the local player's empty offhand")
	}
}

func TestTotemFixtureTracksInitialisedPlayerAndPreservesUnknownCommands(t *testing.T) {
	t.Parallel()
	initialised := &packet.SetLocalPlayerAsInitialised{EntityRuntimeID: 42}
	ordinary := &packet.CommandRequest{CommandLine: "/totemtest unknown"}
	inner := &cameraFixtureConn{read: []packet.Packet{
		initialised, &packet.CommandRequest{CommandLine: "/totemtest offhand"}, ordinary,
	}}
	conn := &totemTestConn{Conn: inner}
	if got, err := conn.ReadPacket(); err != nil || got != initialised {
		t.Fatalf("initialisation intercepted: %v", err)
	}
	if got, err := conn.ReadPacket(); err != nil || got != ordinary {
		t.Fatalf("ordinary command intercepted: %v", err)
	}
	if len(inner.written) == 0 {
		t.Fatal("fixture did not emit an activation")
	}
	remote := totemFixturePackets("remote", 42, mgl32.Vec3{})
	if remote[1].(*packet.ActorEvent).EntityRuntimeID == 42 {
		t.Fatal("remote activation addressed local player")
	}
}

func TestTotemTexturePackOffersOriginalAsymmetricIcon(t *testing.T) {
	t.Parallel()
	pack, err := totemTexturePack()
	if err != nil {
		t.Fatal(err)
	}
	atlas, err := pack.ReadFile("textures/item_texture.json")
	if err != nil || !json.Valid(atlas) {
		t.Fatalf("item atlas: %v", err)
	}
	data, err := pack.ReadFile(totemMarkerPath + ".png")
	if err != nil {
		t.Fatal(err)
	}
	pixels, err := png.Decode(bytes.NewReader(data))
	if err != nil {
		t.Fatal(err)
	}
	if pixels.At(4, 16) == pixels.At(27, 16) {
		t.Fatal("marker sides must be distinguishable during rotation")
	}
	_, _, _, alpha := pixels.At(0, 0).RGBA()
	if alpha != 0 {
		t.Fatal("marker border must remain transparent")
	}
}
