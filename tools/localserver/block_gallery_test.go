package main

import (
	"bytes"
	"encoding/json"
	"io"
	"math"
	"reflect"
	"testing"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/df-mc/dragonfly/server/world/chunk"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// TestBlockGalleryPaletteCoverage guards complete, unique and reproducible state selection.
func TestBlockGalleryPaletteCoverage(t *testing.T) {
	g, err := newBlockGallery(world.DefaultBlockRegistry, "*")
	if err != nil {
		t.Fatal(err)
	}
	if len(g.states) != g.blocks.BlockCount() {
		t.Fatal("gallery omits registered states")
	}
	seen := make(map[uint32]bool)
	for _, s := range g.states {
		if seen[s.RuntimeID] {
			t.Fatalf("duplicate state %d", s.RuntimeID)
		}
		seen[s.RuntimeID] = true
		name, properties, ok := g.blocks.RuntimeIDToState(s.RuntimeID)
		if !ok || name != s.Name || !reflect.DeepEqual(properties, s.Properties) {
			t.Fatalf("state %d was normalized", s.RuntimeID)
		}
		if len(s.Samples) != 4 || len(s.Views) != 3 {
			t.Fatalf("missing samples or views for %s", s.Name)
		}
		if g.states[g.byChunk[s.Chunk]].RuntimeID != s.RuntimeID {
			t.Fatal("cells overlap")
		}
	}
	again, err := newBlockGallery(g.blocks, "*")
	if err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(g.states, again.states) {
		t.Fatal("gallery order changed")
	}
}

// TestBlockGalleryRawStateRoundtrip exercises an unimplemented state through real chunk encoding.
func TestBlockGalleryRawStateRoundtrip(t *testing.T) {
	g, err := newBlockGallery(world.DefaultBlockRegistry, "*")
	if err != nil {
		t.Fatal(err)
	}
	var raw *galleryState
	for i := range g.states {
		b, _ := g.blocks.BlockByRuntimeID(g.states[i].RuntimeID)
		if _, hash := b.Hash(); hash == math.MaxUint64 {
			raw = &g.states[i]
			break
		}
	}
	if raw == nil {
		t.Fatal("registry no longer contains an unimplemented state; update this fixture")
	}
	c := chunk.New(g.blocks, world.Overworld.Range())
	g.GenerateChunk(raw.Chunk, c)
	data := chunk.Encode(c, chunk.NetworkEncoding)
	payload, err := chunk.EncodeLevelChunkPayload(data, nil)
	if err != nil {
		t.Fatal(err)
	}
	decoded, err := chunk.NetworkDecode(g.blocks, payload, len(data.SubChunks), c.Range())
	if err != nil {
		t.Fatal(err)
	}
	g.placements(*raw, func(p cube.Pos, id uint32) {
		got := decoded.Block(uint8(p[0]&15), int16(p[1]), uint8(p[2]&15), 0)
		if got != id {
			t.Fatalf("%s at %v: got ID %d, want %d", raw.Name, p, got, id)
		}
	})
	isolated := raw.Samples[0].Position
	if decoded.Block(uint8((isolated[0]+1)&15), int16(isolated[1]), uint8(isolated[2]&15), 0) != g.blocks.AirRuntimeID() {
		t.Fatal("isolated sample has a neighbour")
	}
}

// TestBlockGalleryFiltersAndOptIn checks family globs, invalid filters and generator exclusivity.
func TestBlockGalleryFiltersAndOptIn(t *testing.T) {
	g, err := newBlockGallery(world.DefaultBlockRegistry, "*stairs, minecraft:*glass*")
	if err != nil {
		t.Fatal(err)
	}
	for _, s := range g.states {
		if !bytes.Contains([]byte(s.Name), []byte("stairs")) && !bytes.Contains([]byte(s.Name), []byte("glass")) {
			t.Fatal("filter admitted unrelated block")
		}
	}
	for _, filter := range []string{"[", "does_not_exist"} {
		if _, err := newBlockGallery(g.blocks, filter); err == nil {
			t.Fatalf("accepted %q", filter)
		}
	}
	for _, extra := range []string{"-terrain-fixture", "-terrain-fixture-generate", "-opaque-overdraw"} {
		if _, err := parseSettings([]string{"-dir", t.TempDir(), "-addr", "127.0.0.1:0", "-block-gallery", extra}, io.Discard); err == nil {
			t.Fatal("accepted conflicting generator")
		}
	}
	conf := server.Config{}
	if err := (settings{}).configureBlockGallery(&conf); err != nil || conf.Generator != nil {
		t.Fatal("gallery must be opt in")
	}
	s := settings{dir: t.TempDir(), blockGallery: true, blockGalleryFilter: "stone"}
	if err := s.configureBlockGallery(&conf); err != nil {
		t.Fatal(err)
	}
	if _, ok := conf.Generator(world.Overworld).(*blockGallery); !ok || conf.RandomTickSpeed >= 0 {
		t.Fatal("gallery did not freeze generation")
	}
	if uc := s.userConfig(); uc.World.SaveData || uc.Players.SaveData {
		t.Fatal("gallery must not save generated cells")
	}
}

// TestBlockGalleryLabelAndActorPackets preserves typed labels and raw actor identities on the wire.
func TestBlockGalleryLabelAndActorPackets(t *testing.T) {
	g, err := newBlockGallery(world.DefaultBlockRegistry, "chest")
	if err != nil {
		t.Fatal(err)
	}
	var wire bytes.Buffer
	(&packet.PrimitiveShapes{Shapes: []protocol.PrimitiveShape{g.label(0)}}).Marshal(protocol.NewWriter(&wire, 0))
	var labels packet.PrimitiveShapes
	labels.Marshal(protocol.NewReader(&wire, 0, true))
	if len(labels.Shapes) != 1 || !labels.Shapes[0].Equal(g.label(0)) {
		t.Fatal("label changed on wire")
	}
	actors := g.actors(g.states[0])
	if len(actors) == 0 {
		t.Fatal("chest state has no actor data")
	}
	for _, actor := range actors {
		wire.Reset()
		actor.Marshal(protocol.NewWriter(&wire, 0))
		var decoded packet.BlockActorData
		decoded.Marshal(protocol.NewReader(&wire, 0, true))
		if decoded.Position != actor.Position || decoded.NBTData["id"] != actor.NBTData["id"] {
			t.Fatal("actor identity changed on wire")
		}
	}
	if _, err := json.Marshal(g.states); err != nil {
		t.Fatal(err)
	}
}

// TestBlockGalleryConnectionOrdersActorDataAfterTerrain leaves other columns and failures alone.
func TestBlockGalleryConnectionOrdersActorDataAfterTerrain(t *testing.T) {
	g, err := newBlockGallery(world.DefaultBlockRegistry, "chest")
	if err != nil {
		t.Fatal(err)
	}
	inner := &cameraFixtureConn{}
	conn := &galleryConn{Conn: inner, gallery: g}
	pk := &packet.SubChunk{Position: protocol.SubChunkPos{0, (galleryGround + 1) >> 4, 0}, SubChunkEntries: []protocol.SubChunkEntry{{Result: protocol.SubChunkResultSuccess}}}
	if err := conn.WritePacket(pk); err != nil {
		t.Fatal(err)
	}
	if len(inner.written) != 1+len(g.actors(g.states[0])) || inner.written[0] != pk {
		t.Fatal("actor data did not follow terrain")
	}
	inner.written = nil
	pk.SubChunkEntries[0].Result = protocol.SubChunkResultChunkNotFound
	if err := conn.WritePacket(pk); err != nil || len(inner.written) != 1 {
		t.Fatal("missing terrain received actor data")
	}
	inner.written = nil
	pk.Dimension = 1
	pk.SubChunkEntries[0].Result = protocol.SubChunkResultSuccess
	if err := conn.WritePacket(pk); err != nil || len(inner.written) != 1 {
		t.Fatal("another dimension received actor data")
	}
	inner.written = nil
	column := &packet.LevelChunk{Position: protocol.ChunkPos(g.states[0].Chunk)}
	if err := conn.WritePacket(column); err != nil || len(inner.written) != 2 || inner.written[0] != column {
		t.Fatal("column label missing or out of order")
	}
}
