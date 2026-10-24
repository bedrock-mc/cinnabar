package main

import (
	"testing"

	"github.com/go-gl/mathgl/mgl32"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestContainerTitlePacketsOpenAnActorBackedMenu(t *testing.T) {
	for mode, filtered := range map[string]bool{"filtered": true, "raw": false} {
		packets := containerTitlePackets(mode, mgl32.Vec3{})
		if len(packets) != 4 {
			t.Fatalf("%s: got %d packets", mode, len(packets))
		}
		actor := packets[1].(*packet.AddActor)
		if actor.EntityMetadata[protocol.EntityDataKeyName] != containerTitleRaw {
			t.Fatalf("%s: name tag %v", mode, actor.EntityMetadata[protocol.EntityDataKeyName])
		}
		if _, ok := actor.EntityMetadata[protocol.EntityDataKeyFilteredName]; ok != filtered {
			t.Fatalf("%s: filtered name present = %v", mode, ok)
		}
		open := packets[2].(*packet.ContainerOpen)
		if open.ContainerEntityUniqueID != actor.EntityUniqueID {
			t.Fatalf("%s: menu opened on %d, actor %d", mode, open.ContainerEntityUniqueID, actor.EntityUniqueID)
		}
	}
	if containerTitlePackets("other", mgl32.Vec3{}) != nil {
		t.Fatal("unknown mode produced packets")
	}
}
