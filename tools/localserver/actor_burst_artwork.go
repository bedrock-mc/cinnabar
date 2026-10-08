package main

import (
	"archive/zip"
	"bytes"
	"encoding/json"
	"fmt"
	"image"
	"image/color"
	"image/png"

	"github.com/go-gl/mathgl/mgl32"
	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

const actorBurstRenderController = "controller.render.cinnabar_actor_burst"

// actorBurstArtworkIdentifier is shared by the offered pack and its spawned actors.
func actorBurstArtworkIdentifier(index int) string {
	return fmt.Sprintf("cinnabar:artwork_fixture_%03d", index)
}

// actorBurstArtworkPack offers original models and distinct texture dimensions through normal pack delivery.
func actorBurstArtworkPack(count int) (*resource.Pack, error) {
	var archive bytes.Buffer
	writer := zip.NewWriter(&archive)
	add := func(name string, data []byte) error {
		file, err := writer.Create(name)
		if err != nil {
			return err
		}
		_, err = file.Write(data)
		return err
	}
	var engine resource.Version
	if _, err := fmt.Sscanf(protocol.CurrentVersion, "%d.%d.%d", &engine[0], &engine[1], &engine[2]); err != nil {
		return nil, err
	}
	version := resource.Version{1, 0, 0}
	name := fmt.Sprintf("actor-artwork-fixture-%d", count)
	manifest, err := json.Marshal(resource.Manifest{
		FormatVersion: 2,
		Header: resource.Header{
			Name: "Actor artwork fixture", Description: "Original synthetic performance artwork",
			UUID: uuid.NewSHA1(uuid.Nil, []byte(name)), Version: version, MinimumGameVersion: engine,
		},
		Modules: []resource.Module{{Type: "resources", Version: version,
			UUID: uuid.NewSHA1(uuid.Nil, []byte(name+"-resources")).String()}},
	})
	if err != nil {
		return nil, err
	}
	if err := add("manifest.json", manifest); err != nil {
		return nil, err
	}
	if err := add("models/entity/actor_burst.geo.json", actorBurstGeometry(0)); err != nil {
		return nil, err
	}
	controller := fmt.Sprintf(`{"format_version":"1.8.0","render_controllers":{%q:{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}`, actorBurstRenderController)
	if err := add("render_controllers/actor_burst.json", []byte(controller)); err != nil {
		return nil, err
	}
	for index := range count {
		texture := fmt.Sprintf("textures/entity/actor_burst_%03d", index)
		entity := fmt.Sprintf(`{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":%q,"materials":{"default":"entity_alphatest"},"textures":{"default":%q},"geometry":{"default":%q},"render_controllers":[%q]}}}`, actorBurstArtworkIdentifier(index), texture, actorBurstGeometryName, actorBurstRenderController)
		if err := add(fmt.Sprintf("entity/actor_burst_%03d.entity.json", index), []byte(entity)); err != nil {
			return nil, err
		}
		pixels := image.NewNRGBA(image.Rect(0, 0, actorBurstSkinSide+index, actorBurstSkinSide))
		for y := range pixels.Bounds().Dy() {
			for x := range pixels.Bounds().Dx() {
				pixels.SetNRGBA(x, y, color.NRGBA{R: byte(index), G: byte(64 + (x/8)*20), B: byte(64 + (y/8)*20), A: 255})
			}
		}
		var encoded bytes.Buffer
		if err := png.Encode(&encoded, pixels); err != nil {
			return nil, err
		}
		if err := add(texture+".png", encoded.Bytes()); err != nil {
			return nil, err
		}
	}
	if err := writer.Close(); err != nil {
		return nil, err
	}
	return resource.ReadBytes(archive.Bytes())
}

// actorBurstArtworkPackets spawns only identifiers declared by the fixture's offered pack.
func actorBurstArtworkPackets(count int, origin mgl32.Vec3) []packet.Packet {
	packets := make([]packet.Packet, 0, count)
	for index := range count {
		packets = append(packets, &packet.AddActor{
			EntityUniqueID: int64(actorBurstFirstID + index), EntityRuntimeID: uint64(actorBurstFirstID + index),
			EntityType: actorBurstArtworkIdentifier(index), Position: actorBurstPosition(index, origin),
			EntityMetadata: actorBurstMetadata(),
		})
	}
	return packets
}

// actorBurstPosition gives all fixture modes the same deterministic formation.
func actorBurstPosition(index int, origin mgl32.Vec3) mgl32.Vec3 {
	const columns, spacing = 16, 1.25
	return origin.Add(mgl32.Vec3{
		float32(index%columns)*spacing - float32(columns-1)*spacing/2,
		0, 5 + float32(index/columns)*spacing,
	})
}
