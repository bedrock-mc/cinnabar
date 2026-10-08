package experience

import (
	"bytes"
	"encoding/binary"
	"hash/crc32"
	"image"
	"image/png"
	"log/slog"
	"math"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"testing"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/item/creative"
	"github.com/df-mc/dragonfly/server/world"
)

// otherCounter is the block of "other", the probe artifact registered a second time under that id.
const otherCounter = "other:counter"

// registration is the registration of the test Experiences. Dragonfly's registries are global and
// server.Config.New freezes them, so it happens once per test process.
var registration struct {
	once sync.Once
	reg  *Registry
	// srv is the server whose New finalized the registries and built the resource pack.
	srv *server.Server
	err error
}

// registered registers the probe artifact and its copy "other", then creates an offline server,
// which finalizes the registries and builds the resource pack. Only the first call does the work.
func registered(t *testing.T) *Registry {
	t.Helper()
	registration.once.Do(func() {
		registration.reg, registration.srv, registration.err = registerTestExperiences()
	})
	if registration.err != nil {
		t.Fatalf("registering the test Experiences: %v", registration.err)
	}
	return registration.reg
}

func registerTestExperiences() (*Registry, *server.Server, error) {
	log := slog.New(slog.DiscardHandler)
	sup, probe, err := StartSupervisor(runtimeBinary, probeDir, log)
	if err != nil {
		return nil, nil, err
	}
	if err := sup.Close(); err != nil {
		return nil, nil, err
	}
	reg, err := Register([]Loaded{probe, renamed(probe, "other")})
	if err != nil {
		return nil, nil, err
	}
	// The server reads its resource folder once, in Config, saves no world or players and listens
	// nowhere.
	dir, err := os.MkdirTemp("", "experience-server-")
	if err != nil {
		return nil, nil, err
	}
	defer os.RemoveAll(dir)
	uc := server.DefaultConfig()
	uc.Server.AuthEnabled = false
	uc.World.SaveData = false
	uc.Players.SaveData = false
	uc.Resources.Folder = filepath.Join(dir, "resources")
	conf, err := uc.Config(log)
	if err != nil {
		return nil, nil, err
	}
	conf.Listeners = nil
	return reg, conf.New(), nil
}

// renamed is loaded as the Experience id, its blocks moved into that namespace.
func renamed(loaded Loaded, id string) Loaded {
	out := loaded
	out.ID = id
	out.Blocks = nil
	for _, def := range loaded.Blocks {
		_, name, _ := strings.Cut(def.ID, ":")
		def.ID = id + ":" + name
		out.Blocks = append(out.Blocks, def)
	}
	return out
}

// testBlock is a breakable block whose every face shows the texture at path.
func testBlock(id, path string) BlockDef {
	return BlockDef{
		ID:          id,
		DisplayName: "Test Block",
		Textures:    []Texture{{Slot: allFaces, Path: path}},
		Mining:      Mining{Breakable: &Breakable{Hardness: 1}},
	}
}

// encodePNG encodes a w×h image as PNG.
func encodePNG(t *testing.T, w, h int) []byte {
	t.Helper()
	var out bytes.Buffer
	if err := png.Encode(&out, image.NewNRGBA(image.Rect(0, 0, w, h))); err != nil {
		t.Fatal(err)
	}
	return out.Bytes()
}

// writeTexture writes a w×h PNG named name into a temporary directory and returns its path.
func writeTexture(t *testing.T, name string, w, h int) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), name)
	if err := os.WriteFile(path, encodePNG(t, w, h), 0o644); err != nil {
		t.Fatal(err)
	}
	return path
}

// padPNG grows the PNG data to size bytes with an ancillary chunk after IHDR, which decoders
// skip.
func padPNG(t *testing.T, data []byte, size int) []byte {
	t.Helper()
	const header = 8 + 4 + 4 + 13 + 4 // the signature and the IHDR chunk
	const overhead = 4 + 4 + 4        // a chunk's length, type and CRC
	n := size - len(data) - overhead
	if n < 0 {
		t.Fatalf("%d bytes of PNG cannot be padded to %d", len(data), size)
	}
	chunk := binary.BigEndian.AppendUint32(nil, uint32(n))
	chunk = append(chunk, "paDd"...)
	chunk = append(chunk, make([]byte, n)...)
	chunk = binary.BigEndian.AppendUint32(chunk, crc32.ChecksumIEEE(chunk[4:]))
	return slices.Concat(data[:header], chunk, data[header:])
}

// assertNothingRegistered checks that no block, item or creative group of loaded reached
// Dragonfly.
func assertNothingRegistered(t *testing.T, loaded []Loaded) {
	t.Helper()
	custom := world.CustomBlocks()
	for _, l := range loaded {
		if slices.ContainsFunc(creative.Groups(), func(g creative.Group) bool { return g.Name == l.ID }) {
			t.Errorf("creative group %q was registered", l.ID)
		}
		for _, def := range l.Blocks {
			if _, ok := custom[def.ID]; ok {
				t.Errorf("block %q was registered", def.ID)
			}
			if _, ok := world.ItemByName(def.ID, 0); ok {
				t.Errorf("item %q was registered", def.ID)
			}
		}
	}
}

// A texture that is not a PNG fails registration, naming its Experience and its file, before
// anything is registered.
func TestBadPNGNamesFile(t *testing.T) {
	path := filepath.Join(t.TempDir(), "counter.png")
	if err := os.WriteFile(path, []byte("not a png"), 0o644); err != nil {
		t.Fatal(err)
	}
	loaded := []Loaded{{ID: "bad", Blocks: []BlockDef{testBlock("bad:counter", path)}}}
	_, err := Register(loaded)
	if err == nil || !strings.Contains(err.Error(), "counter.png") ||
		!strings.Contains(err.Error(), `"bad"`) {
		t.Fatalf(`Register: %v; want an error naming Experience "bad" and counter.png`, err)
	}
	assertNothingRegistered(t, loaded)
}

// One block id in two Experiences, or one Experience loaded twice, fails before anything is
// registered.
func TestDuplicateIDFails(t *testing.T) {
	path := writeTexture(t, "block.png", 1, 1)
	for _, tc := range []struct {
		name   string
		loaded []Loaded
		want   string
	}{
		{
			name: "one block id in two Experiences",
			loaded: []Loaded{
				{ID: "dupa", Blocks: []BlockDef{testBlock("dupa:block", path)}},
				{ID: "dupb", Blocks: []BlockDef{testBlock("dupa:block", path)}},
			},
			want: `"dupa:block"`,
		},
		{
			name: "one Experience twice",
			loaded: []Loaded{
				{ID: "dup", Blocks: []BlockDef{testBlock("dup:block", path)}},
				{ID: "dup", Blocks: []BlockDef{testBlock("dup:block", path)}},
			},
			want: `"dup"`,
		},
	} {
		t.Run(tc.name, func(t *testing.T) {
			_, err := Register(tc.loaded)
			if err == nil || !strings.Contains(err.Error(), tc.want) {
				t.Fatalf("Register: %v; want an error naming %s", err, tc.want)
			}
			assertNothingRegistered(t, tc.loaded)
		})
	}
}

// An Experience may not take the vanilla namespace, whose blocks Dragonfly already holds. It
// fails before anything is registered, the Experiences before it included.
func TestReservedNamespaceFails(t *testing.T) {
	path := writeTexture(t, "block.png", 1, 1)
	loaded := []Loaded{
		{ID: "early", Blocks: []BlockDef{testBlock("early:block", path)}},
		{ID: "minecraft", Blocks: []BlockDef{testBlock("minecraft:counter", path)}},
	}
	_, err := Register(loaded)
	if err == nil || !strings.Contains(err.Error(), `"minecraft"`) ||
		!strings.Contains(err.Error(), "reserved") {
		t.Fatalf(`Register: %v; want an error saying that Experience id "minecraft" is reserved`, err)
	}
	assertNothingRegistered(t, loaded)
}

// Breakable mining keeps its hardness and breaks as fast by hand as with any tool, which is the
// time the client expects; unbreakable mining never breaks; either way the block drops itself. A
// definition without mining is refused, naming its Experience and block.
func TestMiningMapsToBreakInfo(t *testing.T) {
	path := writeTexture(t, "block.png", 1, 1)
	pickaxe := item.NewStack(item.Pickaxe{Tier: item.ToolTierDiamond}, 1)
	for _, tc := range []struct {
		name     string
		mining   Mining
		hardness float64
	}{
		{"breakable", Mining{Breakable: &Breakable{Hardness: 2.5}}, 2.5},
		{"unbreakable", Mining{Unbreakable: &Unbreakable{}}, -1},
	} {
		t.Run(tc.name, func(t *testing.T) {
			def := testBlock("mining:block", path)
			def.Mining = tc.mining
			bt, err := newBlockType("mining", def, map[string]image.Image{})
			if err != nil {
				t.Fatalf("newBlockType: %v", err)
			}
			b := Block{bt}
			info := b.BreakInfo()
			if info.Hardness != tc.hardness {
				t.Errorf("hardness %v, want %v", info.Hardness, tc.hardness)
			}
			hand := block.BreakDuration(b, item.Stack{}, block.BreakContext{})
			if tool := block.BreakDuration(b, pickaxe, block.BreakContext{}); tool != hand {
				t.Errorf("breaking takes %v with a pickaxe and %v by hand; want the same", tool, hand)
			}
			if tc.hardness < 0 && hand != math.MaxInt64 {
				t.Errorf("an unbreakable block breaks by hand in %v", hand)
			}
			drops := info.Drops(item.ToolNone{}, nil)
			if len(drops) != 1 || drops[0].Item() != world.Item(b) || drops[0].Count() != 1 {
				t.Errorf("drops %v, want one of the block itself", drops)
			}
		})
	}
	t.Run("missing", func(t *testing.T) {
		def := testBlock("nomining:block", path)
		def.Mining = Mining{}
		loaded := []Loaded{{ID: "nomining", Blocks: []BlockDef{def}}}
		_, err := Register(loaded)
		if err == nil || !strings.Contains(err.Error(), `"nomining"`) ||
			!strings.Contains(err.Error(), `"nomining:block"`) {
			t.Fatalf("Register: %v; want an error naming the Experience and the block", err)
		}
		assertNothingRegistered(t, loaded)
	})
}

// A texture may be maxTextureSide pixels wide and high, and maxTextureBytes long; one pixel or
// one byte more is refused.
func TestTextureLimits(t *testing.T) {
	dir := t.TempDir()
	small := encodePNG(t, 1, 1)
	for _, tc := range []struct {
		name string
		data []byte
		ok   bool
	}{
		{"largest_image", encodePNG(t, maxTextureSide, maxTextureSide), true},
		{"too_wide", encodePNG(t, maxTextureSide+1, 1), false},
		{"too_high", encodePNG(t, 1, maxTextureSide+1), false},
		{"largest_file", padPNG(t, small, maxTextureBytes), true},
		{"file_too_large", padPNG(t, small, maxTextureBytes+1), false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			path := filepath.Join(dir, tc.name+".png")
			if err := os.WriteFile(path, tc.data, 0o644); err != nil {
				t.Fatal(err)
			}
			_, err := loadTexture(path)
			if tc.ok && err != nil {
				t.Fatalf("loadTexture: %v", err)
			}
			if !tc.ok && err == nil {
				t.Fatal("loadTexture accepted it")
			}
		})
	}
}

// Block data lives in the Experience store; a Block that were an NBTer would carry it into
// chunk NBT.
func TestBlockHasNoNBT(t *testing.T) {
	if _, ok := any(Block{}).(world.NBTer); ok {
		t.Fatal("Block implements world.NBTer")
	}
}

// The probe's block, registered before server.Config.New, is a block and an item of the
// finalized registries, and a custom block that the resource pack builder builds.
func TestBlocksRegisteredBeforeNew(t *testing.T) {
	reg := registered(t)
	want, ok := reg.Lookup(probeCounter)
	if !ok {
		t.Fatalf("Lookup(%q) found nothing", probeCounter)
	}
	if got, ok := world.BlockByName(probeCounter, map[string]any{}); !ok || got != world.Block(want) {
		t.Errorf("BlockByName(%q) = %v, %v; want the registered block", probeCounter, got, ok)
	}
	if got, ok := world.ItemByName(probeCounter, 0); !ok || got != world.Item(want) {
		t.Errorf("ItemByName(%q) = %v, %v; want the registered block", probeCounter, got, ok)
	}
	if _, ok := world.CustomBlocks()[probeCounter].(world.CustomBlockBuildable); !ok {
		t.Errorf("%s is not a buildable custom block, so the resource pack leaves it out",
			probeCounter)
	}
}

// After server.Config.New has added the vanilla creative inventory, each Experience has a
// construction group named after it, shown with its block, and its block is in that group, which
// is how a session finds an item's group.
func TestCreativeContainsBlocks(t *testing.T) {
	reg := registered(t)
	groups := creative.Groups()
	for _, id := range []string{probeCounter, otherCounter} {
		b, ok := reg.Lookup(id)
		if !ok {
			t.Fatalf("Lookup(%q) found nothing", id)
		}
		exp, _, _ := strings.Cut(id, ":")
		g := slices.IndexFunc(groups, func(g creative.Group) bool { return g.Name == exp })
		if g < 0 {
			t.Errorf("no creative group %q", exp)
			continue
		}
		if groups[g].Category != creative.ConstructionCategory() {
			t.Errorf("group %q is in category %d, want construction", exp, groups[g].Category.Uint8())
		}
		if groups[g].Icon.Item() != world.Item(b) {
			t.Errorf("group %q shows %v, want %s", exp, groups[g].Icon.Item(), id)
		}
		if !slices.ContainsFunc(creative.Items(), func(i creative.Item) bool {
			return i.Group == exp && i.Stack.Item() == world.Item(b)
		}) {
			t.Errorf("%s is not in creative group %q", id, exp)
		}
	}
}

// Texture keys come from the whole block id, so one block name in two Experiences gets two keys,
// and every opaque material names a texture that the resource pack carries.
func TestTextureKeysDoNotCollide(t *testing.T) {
	if a, b := textureKey("a:machine", allFaces), textureKey("b:machine", allFaces); a == b {
		t.Fatalf("a:machine and b:machine share texture key %q", a)
	}
	for slot, want := range map[string]string{allFaces: "a.machine.all", "up": "a.machine.up"} {
		if got := textureKey("a:machine", slot); got != want {
			t.Errorf("textureKey(a:machine, %q) = %q, want %q", slot, got, want)
		}
	}
	reg := registered(t)
	probe, _ := reg.Lookup(probeCounter)
	other, _ := reg.Lookup(otherCounter)
	for key := range probe.Textures() {
		if _, ok := other.Textures()[key]; ok {
			t.Errorf("%s and %s share texture key %q", probeCounter, otherCounter, key)
		}
	}
	for _, b := range []Block{probe, other} {
		for slot, material := range b.Properties().Textures {
			encoded := material.Encode()
			if _, ok := b.Textures()[encoded["texture"].(string)]; !ok {
				t.Errorf("%s slot %q uses texture %v, which the pack does not carry",
					b.t.id, slot, encoded["texture"])
			}
			if encoded["render_method"] != "opaque" {
				t.Errorf("%s slot %q renders %v, want opaque", b.t.id, slot, encoded["render_method"])
			}
		}
	}
}

// Every Block of a type is equal and hashes alike with no state hash, and two types differ, so a
// block's hash depends on its type alone.
func TestHashIndependentOfInstance(t *testing.T) {
	reg := registered(t)
	probe, _ := reg.Lookup(probeCounter)
	other, _ := reg.Lookup(otherCounter)
	again, _ := reg.Lookup(probeCounter)
	placed, _ := world.BlockByName(probeCounter, map[string]any{})
	for _, b := range []world.Block{again, placed} {
		if b != world.Block(probe) || world.BlockHash(b) != world.BlockHash(probe) {
			t.Errorf("%v hashes to %d, unlike %v with %d", b, world.BlockHash(b), probe,
				world.BlockHash(probe))
		}
	}
	base, state := probe.Hash()
	if state != 0 {
		t.Errorf("state hash %d, want 0", state)
	}
	if otherBase, _ := other.Hash(); otherBase == base {
		t.Errorf("%s and %s share base hash %d", probeCounter, otherCounter, base)
	}
	if world.BlockRuntimeID(probe) == world.BlockRuntimeID(other) {
		t.Errorf("%s and %s share a runtime id", probeCounter, otherCounter)
	}
}
