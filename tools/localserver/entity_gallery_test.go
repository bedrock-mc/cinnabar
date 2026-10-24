package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"

	"github.com/go-gl/mathgl/mgl32"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestEntityGalleryCoversTheRetailRegistryAcrossPages(t *testing.T) {
	registry := "../../crates/protocol/data/retail_actors_1_26_50.txt"
	gallery, err := loadEntityGallery(registry, "")
	if err != nil {
		t.Fatal(err)
	}
	data, err := os.ReadFile(registry)
	if err != nil {
		t.Fatal(err)
	}
	seen := map[string]bool{}
	for page := 0; ; page++ {
		entries, ok := entityGalleryPage("/entitygallery all "+strconv.Itoa(page), gallery)
		if !ok {
			t.Fatal("valid page rejected")
		}
		if len(entries) == 0 {
			break
		}
		for _, entry := range entries {
			if seen[entry.Identifier] {
				t.Fatalf("repeated %s", entry.Identifier)
			}
			seen[entry.Identifier] = true
		}
	}
	for _, identifier := range strings.Fields(string(data)) {
		if !seen[identifier] {
			t.Errorf("missing %s", identifier)
		}
	}
	if len(seen) != len(gallery) {
		t.Fatal("page traversal lost entries")
	}
}

func TestEntityGalleryPreservesWireTypesAndSpecialSpawns(t *testing.T) {
	page := []entityAppearance{{Identifier: "minecraft:sheep", Name: "baby", Color: 14, Variant: 2, Flags: []uint8{protocol.EntityDataFlagBaby, 100}}, {Identifier: "minecraft:painting", Painting: "Alban"}, {Identifier: "minecraft:item"}}
	packets := entityGalleryPackets(page, mgl32.Vec3{0, 65.62, 0})
	var buffer bytes.Buffer
	packets[0].Marshal(protocol.NewWriter(&buffer, 0))
	var actor packet.AddActor
	actor.Marshal(protocol.NewReader(&buffer, 0, true))
	if actor.EntityMetadata[protocol.EntityDataKeyColorIndex] != uint8(14) || actor.EntityMetadata[protocol.EntityDataKeyVariant] != int32(2) || !actor.EntityMetadata.Flag(protocol.EntityDataKeyFlags, protocol.EntityDataFlagBaby) || !actor.EntityMetadata.Flag(protocol.EntityDataKeyFlagsTwo, 36) {
		t.Fatal("appearance metadata did not survive the wire")
	}
	if packets[2].(*packet.AddPainting).Title != "Alban" || packets[4].(*packet.AddItemActor).Item.Stack.Count != 1 {
		t.Fatal("special spawn path lost its payload")
	}
}

func TestEntityGalleryRejectsStaleAppearancesAndFiltersBeforePaging(t *testing.T) {
	path := filepath.Join(t.TempDir(), "appearances.json")
	if err := os.WriteFile(path, []byte(`[{"identifier":"minecraft:removed","name":"baby"}]`), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := loadEntityGallery("../../crates/protocol/data/retail_actors_1_26_50.txt", path); err == nil {
		t.Fatal("stale appearance accepted")
	}
	gallery := []entityAppearance{{Identifier: "minecraft:pig"}, {Identifier: "minecraft:sheep", Name: "baby"}}
	entries, ok := entityGalleryPage("/entitygallery sheep", gallery)
	if !ok || len(entries) != 1 || entries[0].Name != "baby" {
		t.Fatal("filter lost the selected appearance")
	}
	for _, command := range []string{"/entitygallery sheep -1", "/entitygallery sheep broken", "/other", "/entitygallery sheep 1 extra"} {
		if _, ok := entityGalleryPage(command, gallery); ok {
			t.Fatalf("invalid command accepted: %s", command)
		}
	}
}

func TestEntityGalleryReplacementClearsActorsAndLabels(t *testing.T) {
	ordinary := &packet.CommandRequest{CommandLine: "/give stone"}
	inner := &cameraFixtureConn{read: []packet.Packet{&packet.CommandRequest{CommandLine: "/entitygallery sheep"}, &packet.CommandRequest{CommandLine: "/entitygallery clear"}, ordinary}}
	conn := &galleryConn{Conn: inner, gallery: []entityAppearance{{Identifier: "minecraft:sheep"}}}
	if got, err := conn.ReadPacket(); err != nil || got != ordinary {
		t.Fatal("ordinary command was intercepted")
	}
	if len(inner.written) != 4 {
		t.Fatal("replacement left stale fixture actors")
	}
	for i, pk := range inner.written[2:] {
		if pk.(*packet.RemoveActor).EntityUniqueID != int64(entityGalleryFirstID+i) {
			t.Fatal("removed another fixture's actor")
		}
	}
}
