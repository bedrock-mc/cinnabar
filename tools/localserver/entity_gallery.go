package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"math"
	"os"
	"slices"
	"strconv"
	"strings"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl32"
	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

const entityGalleryFirstID = 9300000
const entityGalleryPageSize = 16
const entityGalleryFallingBlockHeight float32 = 0.98

// entityGalleryFlagWords follows the client's flag layout, including the third-word key alias.
var entityGalleryFlagWords = [...]uint32{protocol.EntityDataKeyFlags, protocol.EntityDataKeyFlagsTwo, protocol.EntityDataKeyArrowShooterID}

// entityAppearance holds typed wire values rather than untyped JSON metadata numbers.
type entityAppearance struct {
	Identifier string                     `json:"identifier"`
	Name       string                     `json:"name"`
	Variant    int32                      `json:"variant"`
	Mark       int32                      `json:"mark_variant"`
	Color      uint8                      `json:"color"`
	Color2     uint8                      `json:"color2"`
	Value      int32                      `json:"value"`
	Flags      []uint8                    `json:"flags"`
	Scale      *float32                   `json:"scale"`
	Width      float32                    `json:"width"`
	Height     float32                    `json:"height"`
	Painting   string                     `json:"painting"`
	Item       *protocol.ItemInstance     `json:"item"`
	Attributes []protocol.AttributeValue  `json:"attributes"`
	Properties protocol.EntityProperties  `json:"properties"`
	Armour     *packet.MobArmourEquipment `json:"armour"`
	Cloud      *entityCloudAppearance     `json:"cloud"`
	Yaw        *float32                   `json:"yaw"`
	AttachFace *uint8                     `json:"attach_face"`
	Peek       *int32                     `json:"peek"`
}

// entityCloudAppearance supplies the typed state needed to display a lingering cloud.
type entityCloudAppearance struct {
	Radius   float32 `json:"radius"`
	Duration int32   `json:"duration"`
	Particle int32   `json:"particle"`
	Color    int32   `json:"color"`
}

// loadEntityGallery derives default cases from the registry and rejects stale appearance identities.
func loadEntityGallery(registry, appearances string) ([]entityAppearance, error) {
	data, err := os.ReadFile(registry)
	if err != nil {
		return nil, err
	}
	identifiers := strings.Fields(string(data))
	if bytes.HasPrefix(data, []byte("EFACT001")) {
		if len(data) < 40 {
			return nil, fmt.Errorf("truncated entity facts registry")
		}
		payload, checksum := data[:len(data)-32], data[len(data)-32:]
		hash := sha256.Sum256(payload)
		if !bytes.Equal(hash[:], checksum) {
			return nil, fmt.Errorf("invalid entity facts checksum")
		}
		var facts struct {
			Records []struct {
				Identifier string `json:"identifier"`
			} `json:"records"`
		}
		if err := json.Unmarshal(payload[8:], &facts); err != nil {
			return nil, err
		}
		identifiers = nil
		for _, record := range facts.Records {
			identifiers = append(identifiers, record.Identifier)
		}
	}
	known := map[string]bool{}
	var gallery []entityAppearance
	for _, id := range identifiers {
		if !strings.HasPrefix(id, "minecraft:") || known[id] {
			return nil, fmt.Errorf("invalid or duplicate actor %q", id)
		}
		known[id] = true
		gallery = append(gallery, entityAppearance{Identifier: id, Name: "default"})
	}
	if len(gallery) == 0 {
		return nil, fmt.Errorf("empty actor registry")
	}
	if appearances != "" {
		data, err := os.ReadFile(appearances)
		if err != nil {
			return nil, err
		}
		var extra []entityAppearance
		decoder := json.NewDecoder(strings.NewReader(string(data)))
		decoder.DisallowUnknownFields()
		if err := decoder.Decode(&extra); err != nil {
			return nil, err
		}
		for _, appearance := range extra {
			if !known[appearance.Identifier] || appearance.Name == "" || appearance.Name == "default" {
				return nil, fmt.Errorf("invalid appearance %q for %q", appearance.Name, appearance.Identifier)
			}
			for _, flag := range appearance.Flags {
				if int(flag) >= len(entityGalleryFlagWords)*64 {
					return nil, fmt.Errorf("flag %d exceeds the metadata flag words", flag)
				}
			}
			gallery = append(gallery, appearance)
		}
	}
	slices.SortFunc(gallery, func(a, b entityAppearance) int {
		if order := strings.Compare(a.Identifier, b.Identifier); order != 0 {
			return order
		}
		return strings.Compare(a.Name, b.Name)
	})
	for i := 1; i < len(gallery); i++ {
		if gallery[i].Identifier == gallery[i-1].Identifier && gallery[i].Name == gallery[i-1].Name {
			return nil, fmt.Errorf("duplicate appearance %s/%s", gallery[i].Identifier, gallery[i].Name)
		}
	}
	return gallery, nil
}

// entityGalleryListener scopes the fixture and its synthetic actors to an opted-in server.
func entityGalleryListener(inner func(server.Config) (server.Listener, error), gallery []entityAppearance) func(server.Config) (server.Listener, error) {
	return func(conf server.Config) (server.Listener, error) {
		listener, err := inner(conf)
		if err != nil {
			return nil, err
		}
		return &entityGalleryPacketListener{Listener: listener, gallery: gallery}, nil
	}
}

type entityGalleryPacketListener struct {
	server.Listener
	gallery []entityAppearance
}

// Accept gives each viewer an independent page and replacement state.
func (l *entityGalleryPacketListener) Accept() (session.Conn, error) {
	conn, err := l.Listener.Accept()
	if err != nil {
		return nil, err
	}
	return &entityGalleryPacketConn{Conn: conn, gallery: l.gallery}, nil
}

// Disconnect returns the original connection to its owning listener.
func (l *entityGalleryPacketListener) Disconnect(conn session.Conn, reason string) error {
	if wrapped, ok := conn.(*entityGalleryPacketConn); ok {
		conn = wrapped.Conn
	}
	return l.Listener.Disconnect(conn, reason)
}

type entityGalleryPacketConn struct {
	session.Conn
	gallery  []entityAppearance
	position mgl32.Vec3
	count    int
	players  []protocol.PlayerListEntry
}

// ReadPacket replaces only this viewer's gallery and leaves other commands untouched.
func (c *entityGalleryPacketConn) ReadPacket() (packet.Packet, error) {
	for {
		pk, err := c.Conn.ReadPacket()
		if err != nil {
			return pk, err
		}
		if input, ok := pk.(*packet.PlayerAuthInput); ok {
			c.position = input.Position
		}
		request, ok := pk.(*packet.CommandRequest)
		if !ok {
			return pk, nil
		}
		if peek, recognized := entityGalleryPeek(request.CommandLine); recognized {
			for i := 0; i < c.count; i++ {
				update := &packet.SetActorData{EntityRuntimeID: uint64(entityGalleryFirstID + i*2), EntityMetadata: protocol.EntityMetadata{protocol.EntityDataKeyPeekID: peek}}
				if err := c.Conn.WritePacket(update); err != nil {
					return nil, err
				}
			}
			continue
		}
		page, recognized := entityGalleryPage(request.CommandLine, c.gallery)
		if !recognized {
			return pk, nil
		}
		for i := 0; i < c.count*2; i++ {
			if err := c.Conn.WritePacket(&packet.RemoveActor{EntityUniqueID: int64(entityGalleryFirstID + i)}); err != nil {
				return nil, err
			}
		}
		if len(c.players) > 0 {
			if err := c.Conn.WritePacket(&packet.PlayerList{Entries: c.players}); err != nil {
				return nil, err
			}
		}
		c.players = nil
		for _, fixture := range entityGalleryPackets(page, c.position) {
			if err := c.Conn.WritePacket(fixture); err != nil {
				return nil, err
			}
			if list, ok := fixture.(*packet.PlayerList); ok {
				for _, entry := range list.Entries {
					c.players = append(c.players, protocol.PlayerListEntry{ActionType: protocol.PlayerListActionRemove, UUID: entry.UUID})
				}
			}
		}
		c.count = len(page)
	}
}

// entityGalleryPeek parses `/entitygallery peek N`, which sends the shulker peek data item to the shown actors.
func entityGalleryPeek(command string) (int32, bool) {
	fields := strings.Fields(strings.TrimPrefix(command, "/"))
	if len(fields) != 3 || fields[0] != "entitygallery" || fields[1] != "peek" {
		return 0, false
	}
	peek, err := strconv.ParseInt(fields[2], 10, 32)
	return int32(peek), err == nil
}

// entityGalleryPage filters the sorted catalog before selecting a zero-based page.
func entityGalleryPage(command string, gallery []entityAppearance) ([]entityAppearance, bool) {
	fields := strings.Fields(strings.TrimPrefix(command, "/"))
	if len(fields) == 0 || fields[0] != "entitygallery" || len(fields) > 3 {
		return nil, false
	}
	if len(fields) == 2 && fields[1] == "clear" {
		return nil, true
	}
	filter, page := "", 0
	if len(fields) >= 2 && fields[1] != "all" {
		filter = fields[1]
	}
	if len(fields) == 3 {
		parsed, err := strconv.Atoi(fields[2])
		if err != nil || parsed < 0 {
			return nil, false
		}
		page = parsed
	}
	var matches []entityAppearance
	for _, appearance := range gallery {
		if strings.Contains(appearance.Identifier+"/"+appearance.Name, filter) {
			matches = append(matches, appearance)
		}
	}
	if page > len(matches)/entityGalleryPageSize {
		return nil, true
	}
	start := min(page*entityGalleryPageSize, len(matches))
	return matches[start:min(start+entityGalleryPageSize, len(matches))], true
}

// entityGalleryPackets uses separate zero-scale labels so invisible actors can still be located.
func entityGalleryPackets(page []entityAppearance, origin mgl32.Vec3) []packet.Packet {
	var packets []packet.Packet
	for i, appearance := range page {
		id := int64(entityGalleryFirstID + i*2)
		columns := min(4, len(page))
		position := origin.Add(mgl32.Vec3{(float32(i%4) - float32(columns-1)/2) * 5, -1.62, 7 + float32(i/4)*7})
		metadata := appearance.metadata()
		yaw := float32(180)
		if appearance.Yaw != nil {
			yaw = *appearance.Yaw
		}
		var actor packet.Packet = &packet.AddActor{EntityUniqueID: id, EntityRuntimeID: uint64(id), EntityType: appearance.Identifier, Position: position.Add(mgl32.Vec3{0, appearance.spawnOffset(), 0}), Yaw: yaw, HeadYaw: yaw, BodyYaw: yaw, EntityMetadata: metadata, Attributes: appearance.Attributes, EntityProperties: appearance.Properties}
		if appearance.Identifier == "minecraft:painting" {
			title := appearance.Painting
			if title == "" {
				title = entityGalleryPaintingMotives[0].Title
			}
			center := position.Add(mgl32.Vec3{0, entityGalleryPaintingHeight(title) / 2, 0})
			actor = &packet.AddPainting{EntityUniqueID: id, EntityRuntimeID: uint64(id), Position: center, Direction: 2, Title: title}
		} else if appearance.Identifier == "minecraft:item" {
			stack := appearance.Item
			if stack == nil {
				runtimeID, meta, _ := world.ItemRuntimeID(item.Diamond{})
				stack = &protocol.ItemInstance{StackNetworkID: 1, Stack: protocol.ItemStack{ItemType: protocol.ItemType{NetworkID: runtimeID, MetadataValue: uint32(meta)}, Count: 1}}
			}
			actor = &packet.AddItemActor{EntityUniqueID: id, EntityRuntimeID: uint64(id), Position: position.Add(mgl32.Vec3{0, appearance.spawnOffset(), 0}), Item: *stack, EntityMetadata: metadata}
		} else if appearance.Identifier == "minecraft:player" {
			identity := entityGalleryUUID(id)
			packets = append(packets, &packet.PlayerList{Entries: []protocol.PlayerListEntry{{ActionType: protocol.PlayerListActionAdd, UUID: identity, EntityUniqueID: id, Username: "Gallery player", Skin: actorBurstSkin(0, 0)}}})
			actor = &packet.AddPlayer{UUID: identity, Username: "Gallery player", EntityRuntimeID: uint64(id), Position: position, EntityMetadata: metadata, EntityProperties: appearance.Properties, AbilityData: protocol.AbilityData{EntityUniqueID: id}}
		}
		label := protocol.EntityMetadata{protocol.EntityDataKeyFlags: int64(0), protocol.EntityDataKeyName: appearance.Identifier + "\n" + appearance.Name, protocol.EntityDataKeyScale: float32(0), protocol.EntityDataKeyWidth: float32(0), protocol.EntityDataKeyHeight: float32(0), protocol.EntityDataKeyAlwaysShowNameTag: uint8(1)}
		label.SetFlag(protocol.EntityDataKeyFlags, protocol.EntityDataFlagShowName)
		label.SetFlag(protocol.EntityDataKeyFlags, protocol.EntityDataFlagAlwaysShowName)
		packets = append(packets, actor, &packet.AddActor{EntityUniqueID: id + 1, EntityRuntimeID: uint64(id + 1), EntityType: "minecraft:armor_stand", Position: position.Add(mgl32.Vec3{0, 3, 0}), EntityMetadata: label})
		if appearance.Identifier == "minecraft:player" && len(appearance.Attributes) > 0 {
			attributes := make([]protocol.Attribute, len(appearance.Attributes))
			for i, value := range appearance.Attributes {
				attributes[i] = protocol.Attribute{AttributeValue: value, DefaultMin: value.Min, DefaultMax: value.Max, Default: value.Value}
			}
			packets = append(packets, &packet.UpdateAttributes{EntityRuntimeID: uint64(id), Attributes: attributes})
		}
		if appearance.Armour != nil {
			armour := *appearance.Armour
			armour.EntityRuntimeID = uint64(id)
			packets = append(packets, &armour)
		}
	}
	return packets
}

// entityGalleryUUID isolates fixture player identities from account and actor-burst identities.
func entityGalleryUUID(id int64) uuid.UUID {
	return uuid.NewSHA1(uuid.Nil, []byte(fmt.Sprintf("entity-gallery-%d", id)))
}

// spawnOffset compensates for the client's spawn normalization, including effective block size.
func (a entityAppearance) spawnOffset() float32 {
	switch a.Identifier {
	case "minecraft:boat", "minecraft:chest_boat":
		return 0.375
	case "minecraft:falling_block":
		baseOffset := float32(0.5)
		height, scale := entityGalleryFallingBlockHeight, float32(1)
		if a.Height > 0 && !math.IsInf(float64(a.Height), 0) {
			height = a.Height
		}
		if a.Scale != nil && *a.Scale >= 0 && !math.IsInf(float64(*a.Scale), 0) {
			scale = *a.Scale
		}
		offset := baseOffset * height * scale / entityGalleryFallingBlockHeight
		if !math.IsInf(float64(offset), 0) && !math.IsNaN(float64(offset)) {
			return offset
		}
		return baseOffset
	case "minecraft:item":
		return 0.125
	default:
		return 0
	}
}

// metadata preserves each field's network type and places flags in their metadata word.
func (a entityAppearance) metadata() protocol.EntityMetadata {
	metadata := protocol.NewEntityMetadata()
	for _, key := range entityGalleryFlagWords {
		metadata[key] = int64(0)
	}
	metadata[protocol.EntityDataKeyVariant], metadata[protocol.EntityDataKeyMarkVariant] = a.Variant, a.Mark
	metadata[protocol.EntityDataKeyColorIndex], metadata[protocol.EntityDataKeyColorTwoIndex] = a.Color, a.Color2
	metadata[protocol.EntityDataKeyValue] = a.Value
	for _, flag := range a.Flags {
		metadata.SetFlag(entityGalleryFlagWords[flag/64], flag%64)
	}
	if a.Scale != nil {
		metadata[protocol.EntityDataKeyScale] = *a.Scale
	}
	if a.Width > 0 {
		metadata[protocol.EntityDataKeyWidth] = a.Width
	}
	if a.Height > 0 {
		metadata[protocol.EntityDataKeyHeight] = a.Height
	}
	if a.AttachFace != nil {
		metadata[protocol.EntityDataKeyAttachFace] = *a.AttachFace
	}
	if a.Peek != nil {
		metadata[protocol.EntityDataKeyPeekID] = *a.Peek
	}
	if a.Cloud != nil {
		metadata[protocol.EntityDataKeyDataRadius] = a.Cloud.Radius
		metadata[protocol.EntityDataKeyDataDuration] = a.Cloud.Duration
		metadata[protocol.EntityDataKeyDataParticle] = a.Cloud.Particle
		metadata[protocol.EntityDataKeyEffectColor] = a.Cloud.Color
		metadata[protocol.EntityDataKeyDataChangeRate] = float32(0)
	}
	return metadata
}
