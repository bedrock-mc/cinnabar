package main

import (
	"archive/zip"
	"bytes"
	"encoding/json"
	"fmt"
	"image"
	"image/color"

	"github.com/df-mc/dragonfly/server/item/category"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

const (
	baseTextureIconID = "fixture:base_texture_icon"
	// baseTextureIconPath names base-game block art that the fixture pack does not ship.
	baseTextureIconPath = "textures/blocks/beacon"
)

// baseTextureIcon is a server item whose pack icon points at a base-game block texture, as
// server menu items often do.
type baseTextureIcon struct{}

var _ world.CustomItem = baseTextureIcon{}

func (baseTextureIcon) EncodeItem() (string, int16) { return baseTextureIconID, 0 }
func (baseTextureIcon) Name() string                { return "Base Texture Icon" }
func (baseTextureIcon) Category() category.Category { return category.Items() }

// Texture is the generated pack's placeholder, which the fixture pack's catalog entry outranks.
func (baseTextureIcon) Texture() image.Image {
	pixels := image.NewNRGBA(image.Rect(0, 0, 16, 16))
	for index := range pixels.Pix {
		pixels.Pix[index] = 255
	}
	pixels.SetNRGBA(0, 0, color.NRGBA{A: 255})
	return pixels
}

// registerBaseTextureIcon adds the item; it must run before the server builds its worlds.
func registerBaseTextureIcon() {
	world.RegisterItem(baseTextureIcon{})
}

// baseTextureIconPack maps the item's icon key to the base-game texture path without the image.
func baseTextureIconPack() (*resource.Pack, error) {
	catalog, err := json.Marshal(map[string]any{
		"resource_pack_name": "base_texture_icon_fixture",
		"texture_name":       "atlas.items",
		"texture_data": map[string]any{
			baseTextureIconID: map[string]string{"textures": baseTextureIconPath},
		},
	})
	if err != nil {
		return nil, err
	}
	return fixtureResourcePack("Base texture icon fixture", "cinnabar-base-texture-icon-fixture",
		[]fixtureFile{{"textures/item_texture.json", catalog}})
}

// fixtureFile is one archived pack file.
type fixtureFile struct {
	path string
	data []byte
}

// fixtureResourcePack archives `files` in order under a resource manifest named `name` and
// identified by `seed`, for the current protocol's engine version.
func fixtureResourcePack(name, seed string, files []fixtureFile) (*resource.Pack, error) {
	var engine resource.Version
	if _, err := fmt.Sscanf(protocol.CurrentVersion, "%d.%d.%d", &engine[0], &engine[1], &engine[2]); err != nil {
		return nil, err
	}
	version := resource.Version{1, 0, 0}
	manifest, err := json.Marshal(resource.Manifest{
		FormatVersion: 2,
		Header: resource.Header{Name: name, Description: "Original fixture pack",
			UUID: uuid.NewSHA1(uuid.Nil, []byte(seed)), Version: version, MinimumGameVersion: engine},
		Modules: []resource.Module{{Type: "resources", Version: version,
			UUID: uuid.NewSHA1(uuid.Nil, []byte(seed+"-resources")).String()}},
	})
	if err != nil {
		return nil, err
	}
	var archive bytes.Buffer
	writer := zip.NewWriter(&archive)
	write := func(path string, data []byte) error {
		file, err := writer.Create(path)
		if err != nil {
			return err
		}
		_, err = file.Write(data)
		return err
	}
	if err := write("manifest.json", manifest); err != nil {
		return nil, err
	}
	for _, file := range files {
		if err := write(file.path, file.data); err != nil {
			return nil, err
		}
	}
	if err := writer.Close(); err != nil {
		return nil, err
	}
	return resource.ReadBytes(archive.Bytes())
}
