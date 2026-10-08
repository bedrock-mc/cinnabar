package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"image/png"
	"io"
	"testing"

	"github.com/go-gl/mathgl/mgl32"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestActorBurstArtworkPackMatchesSpawnedActors(t *testing.T) {
	const count = 4
	pack, err := actorBurstArtworkPack(count)
	if err != nil {
		t.Fatal(err)
	}
	other, err := actorBurstArtworkPack(count)
	if err != nil {
		t.Fatal(err)
	}
	left, right := make([]byte, pack.Len()), make([]byte, other.Len())
	if _, err := pack.ReadAt(left, 0); err != nil {
		t.Fatal(err)
	}
	if _, err := other.ReadAt(right, 0); err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(left, right) {
		t.Fatal("resource pack is not deterministic")
	}
	model, err := pack.ReadFile("models/entity/actor_burst.geo.json")
	if err != nil || !json.Valid(model) {
		t.Fatalf("geometry: %v", err)
	}
	actors := actorBurstPackets(count, "artwork", mgl32.Vec3{0, 64, 0})
	if len(actors) != count {
		t.Fatal("artwork burst must not create player-list entries")
	}
	for index, actor := range actors {
		var wire bytes.Buffer
		actor.Marshal(protocol.NewWriter(&wire, 0))
		var decoded packet.AddActor
		decoded.Marshal(protocol.NewReader(&wire, 0, true))
		if decoded.EntityType != actorBurstArtworkIdentifier(index) || decoded.EntityRuntimeID != uint64(decoded.EntityUniqueID) {
			t.Fatal("invalid artwork identity")
		}
		entity, err := pack.ReadFile(fmt.Sprintf("entity/actor_burst_%03d.entity.json", index))
		if err != nil || !json.Valid(entity) || !bytes.Contains(entity, []byte(decoded.EntityType)) {
			t.Fatalf("unoffered actor %q: %v", decoded.EntityType, err)
		}
		texture, err := pack.ReadFile(fmt.Sprintf("textures/entity/actor_burst_%03d.png", index))
		if err != nil {
			t.Fatal(err)
		}
		pixels, err := png.Decode(bytes.NewReader(texture))
		if err != nil || pixels.Bounds().Dx() != actorBurstSkinSide+index || pixels.Bounds().Dy() != actorBurstSkinSide {
			t.Fatalf("texture %d: %v", index, err)
		}
	}
	removed := actorBurstClear(count, "artwork")
	if len(removed) != count {
		t.Fatal("artwork clear touched player-list entries")
	}
}

func TestActorBurstArtworkRequiresOfferedCount(t *testing.T) {
	request := &packet.CommandRequest{CommandLine: "/actorburst 3 artwork"}
	for _, offered := range []int{0, 2} {
		inner := &cameraFixtureConn{read: []packet.Packet{request}}
		conn := &actorBurstConn{Conn: inner, artwork: offered}
		if got, err := conn.ReadPacket(); err != nil || got != request || len(inner.written) != 0 {
			t.Fatal("spawned unoffered artwork")
		}
	}
	ordinary := &packet.CommandRequest{CommandLine: "/other"}
	inner := &cameraFixtureConn{read: []packet.Packet{request, &packet.CommandRequest{CommandLine: "/actorburst clear"}, ordinary}}
	conn := &actorBurstConn{Conn: inner, artwork: 3}
	if got, err := conn.ReadPacket(); err != nil || got != ordinary || len(inner.written) != 6 {
		t.Fatal("offered artwork did not spawn and clear")
	}
	for _, count := range []int{-1, actorBurstLimit + 1} {
		if _, err := parseSettings([]string{"-dir", t.TempDir(), "-addr", "127.0.0.1:19132", "-actor-burst-artwork", fmt.Sprint(count)}, io.Discard); err == nil {
			t.Fatal("accepted invalid pack size")
		}
	}
}
