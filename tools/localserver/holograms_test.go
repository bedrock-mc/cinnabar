package main

import (
	"bytes"
	"reflect"
	"strings"
	"testing"

	"github.com/go-gl/mathgl/mgl32"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestHologramFixturePreservesStaticLinesAndUpdatesHalfTheActors(t *testing.T) {
	origin := mgl32.Vec3{0, 65, 0}
	spawn := hologramSpawn(origin)
	if len(spawn) != hologramCount || !reflect.DeepEqual(spawn, hologramSpawn(origin)) {
		t.Fatal("spawn is not a deterministic full crowd")
	}
	seenLines := make(map[int]bool)
	var buffer bytes.Buffer
	for index, pk := range spawn {
		buffer.Reset()
		pk.Marshal(protocol.NewWriter(&buffer, 0))
		var actor packet.AddActor
		actor.Marshal(protocol.NewReader(&buffer, 0, true))
		if actor.EntityRuntimeID != uint64(hologramFirstID+index) || actor.Position[2] <= origin[2] || actor.EntityMetadata[protocol.EntityDataKeyScale] != float32(0) {
			t.Fatal("actor identity, position or marker scale was lost")
		}
		text := actor.EntityMetadata[protocol.EntityDataKeyName].(string)
		seenLines[len(strings.Split(text, "\n"))] = true
		if !strings.Contains(text, "§") || (index%2 == 0 && text != hologramText(index, 100)) {
			t.Fatal("static text changed or formatting was lost")
		}
	}
	if len(seenLines) != hologramMaxLines || !strings.Contains(hologramText(0, 0), "§k") || !strings.Contains(hologramText(0, 0), "§l") {
		t.Fatal("missing line counts or effects")
	}
	updates := hologramUpdates(1)
	if len(updates) != hologramCount/2 {
		t.Fatal("wrong update count")
	}
	for _, pk := range updates {
		update := pk.(*packet.SetActorData)
		index := int(update.EntityRuntimeID) - hologramFirstID
		if index%2 != 1 || len(update.EntityMetadata) != 1 || update.EntityMetadata[protocol.EntityDataKeyName] == hologramText(index, 0) {
			t.Fatal("update changed a static actor or unrelated metadata")
		}
	}
}

func TestHologramConnectionFreezesResetsAndClearsItsOwnActors(t *testing.T) {
	input := &packet.PlayerAuthInput{Position: mgl32.Vec3{0, 65, 0}}
	ordinary := &packet.CommandRequest{CommandLine: "/give stone"}
	inner := &cameraFixtureConn{read: []packet.Packet{input, input,
		&packet.CommandRequest{CommandLine: "/holograms freeze"}, input,
		&packet.CommandRequest{CommandLine: "/holograms clear"}, ordinary,
	}}
	conn := &hologramConn{Conn: inner, initial: true}
	for range 3 {
		if got, err := conn.ReadPacket(); err != nil || got != input {
			t.Fatal("input was swallowed")
		}
	}
	if !conn.frozen || conn.tick != 0 {
		t.Fatal("freeze did not reset the animation")
	}
	if got, err := conn.ReadPacket(); err != nil || got != ordinary || conn.spawned {
		t.Fatal("clear swallowed an ordinary command or kept the fixture")
	}
	want := hologramCount + hologramCount/2 + hologramCount*2 + hologramCount
	if len(inner.written) != want {
		t.Fatalf("got %d packets, want %d", len(inner.written), want)
	}
	for _, pk := range inner.written[len(inner.written)-hologramCount:] {
		id := pk.(*packet.RemoveActor).EntityUniqueID
		if id < hologramFirstID || id >= hologramFirstID+hologramCount {
			t.Fatal("cleared another actor")
		}
	}
}
