package main

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
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

func TestEntityGalleryIncludesDedicatedSpawnTypesFromNativeFacts(t *testing.T) {
	paths, err := filepath.Glob("../../crates/assets/data/entity-facts-*.bin")
	if err != nil || len(paths) == 0 {
		t.Fatal("missing pinned entity facts registry")
	}
	for _, path := range paths {
		gallery, err := loadEntityGallery(path, "")
		if err != nil {
			t.Fatal(err)
		}
		for _, identifier := range []string{"minecraft:item", "minecraft:painting", "minecraft:falling_block"} {
			page, _ := entityGalleryPage("/entitygallery "+identifier, gallery)
			if len(page) != 1 {
				t.Errorf("missing native actor %s", identifier)
			}
		}
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
	centered := entityGalleryPackets(page[:1], mgl32.Vec3{20, 65, 0})[0].(*packet.AddActor)
	if centered.Position[0] != 20 {
		t.Fatal("single-actor page is outside the viewer's center")
	}
	page[0].Attributes = []protocol.AttributeValue{{Name: "minecraft:health", Value: 8, Max: 8}}
	page[0].Armour = &packet.MobArmourEquipment{EntityRuntimeID: 123}
	packets = entityGalleryPackets(page[:1], mgl32.Vec3{})
	if packets[0].(*packet.AddActor).Attributes[0].Value != 8 || packets[2].(*packet.MobArmourEquipment).EntityRuntimeID != entityGalleryFirstID {
		t.Fatal("attribute or equipment case addressed the wrong actor")
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

func TestEntityGallerySpawnHeightsMatchEntityFacts(t *testing.T) {
	paths, err := filepath.Glob("../../crates/assets/data/entity-facts-*.bin")
	if err != nil || len(paths) == 0 {
		t.Fatal("missing pinned entity facts registry")
	}
	for _, path := range paths {
		data, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		var facts struct {
			Records []struct {
				Identifier string `json:"identifier"`
				Fact       struct {
					NativeClass    string  `json:"native_class"`
					NetworkYOffset float32 `json:"network_y_offset"`
				} `json:"fact"`
			} `json:"records"`
		}
		if err := json.Unmarshal(data[8:len(data)-32], &facts); err != nil {
			t.Fatal(err)
		}
		for _, record := range facts.Records {
			offset := float32(0)
			if record.Fact.NativeClass == "Boat" || record.Fact.NativeClass == "FallingBlock" {
				offset = record.Fact.NetworkYOffset
			}
			packets := entityGalleryPackets([]entityAppearance{{Identifier: record.Identifier}}, mgl32.Vec3{0, 65.62, 0})
			for _, pk := range packets {
				if actor, ok := pk.(*packet.AddActor); ok && actor.EntityRuntimeID == entityGalleryFirstID {
					if want := float32(65.62) - float32(1.62) + offset; actor.Position[1] != want {
						t.Errorf("%s: network height %v, want %v from %s", record.Identifier, actor.Position[1], want, path)
					}
				}
			}
		}
	}
}

func TestEntityGallerySupportsEveryProtocolFlag(t *testing.T) {
	data, err := os.ReadFile("../../crates/protocol-ids/data/actor-data-extensions.json")
	if err != nil {
		t.Fatal(err)
	}
	var extensions map[string]uint32
	if err := json.Unmarshal(data, &extensions); err != nil {
		t.Fatal(err)
	}
	third, ok := extensions["FlagsThird"]
	if !ok {
		t.Fatal("missing third flag word definition")
	}
	keys := []uint32{protocol.EntityDataKeyFlags, protocol.EntityDataKeyFlagsTwo, third}
	if protocol.EntityDataFlagCount > len(keys)*64 {
		t.Fatal("protocol flags exceed gallery metadata words")
	}
	var appearances []entityAppearance
	for flag := 0; flag < protocol.EntityDataFlagCount; flag++ {
		appearances = append(appearances, entityAppearance{Identifier: "minecraft:sheep", Name: strconv.Itoa(flag), Flags: []uint8{uint8(flag)}})
	}
	path := filepath.Join(t.TempDir(), "appearances.json")
	data, err = json.Marshal(appearances)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, data, 0600); err != nil {
		t.Fatal(err)
	}
	gallery, err := loadEntityGallery("../../crates/protocol/data/retail_actors_1_26_50.txt", path)
	if err != nil {
		t.Fatal(err)
	}
	for _, appearance := range gallery {
		if len(appearance.Flags) == 0 {
			continue
		}
		var buffer bytes.Buffer
		entityGalleryPackets([]entityAppearance{appearance}, mgl32.Vec3{})[0].Marshal(protocol.NewWriter(&buffer, 0))
		var actor packet.AddActor
		actor.Marshal(protocol.NewReader(&buffer, 0, true))
		flag := appearance.Flags[0]
		for word, key := range keys {
			want := int64(0)
			if word == int(flag)/64 {
				want = int64(uint64(1) << (flag % 64))
			}
			if got := actor.EntityMetadata[key]; got != want {
				t.Errorf("flag %d, word %d: got %v, want %d", flag, word, got, want)
			}
		}
	}
}

func TestEntityGalleryPreservesPlayerPropertiesAndAttributes(t *testing.T) {
	properties := protocol.EntityProperties{
		IntegerProperties: []protocol.IntegerEntityProperty{{Index: 2, Value: 7}},
		FloatProperties:   []protocol.FloatEntityProperty{{Index: 4, Value: 0.25}},
	}
	attributes := []protocol.AttributeValue{{Name: "minecraft:health", Value: 0, Min: 0, Max: 20}, {Name: "minecraft:movement", Value: 0.1, Min: 0, Max: 1}}
	packets := entityGalleryPackets([]entityAppearance{{Identifier: "minecraft:player", Properties: properties, Attributes: attributes}, {Identifier: "minecraft:player"}}, mgl32.Vec3{})
	spawned, updated := false, false
	for _, pk := range packets {
		var buffer bytes.Buffer
		pk.Marshal(protocol.NewWriter(&buffer, 0))
		switch pk.(type) {
		case *packet.AddPlayer:
			var actor packet.AddPlayer
			actor.Marshal(protocol.NewReader(&buffer, 0, true))
			if actor.EntityRuntimeID == entityGalleryFirstID {
				spawned = true
				if !reflect.DeepEqual(actor.EntityProperties, properties) {
					t.Errorf("player properties lost: %+v", actor.EntityProperties)
				}
			}
		case *packet.UpdateAttributes:
			var update packet.UpdateAttributes
			update.Marshal(protocol.NewReader(&buffer, 0, true))
			if update.EntityRuntimeID != entityGalleryFirstID || !spawned || updated {
				t.Fatal("player attributes must follow their spawn exactly once")
			}
			updated = true
			if len(update.Attributes) != len(attributes) {
				t.Fatalf("player attributes lost: %+v", update.Attributes)
			}
			for i, attribute := range update.Attributes {
				if attribute.AttributeValue != attributes[i] {
					t.Errorf("player attribute changed: %+v", attribute)
				}
			}
		}
	}
	if !spawned || !updated {
		t.Fatal("player spawn or configured attribute update missing")
	}
}
