package localserver

import (
	"archive/zip"
	"bytes"
	"encoding/json"
	"fmt"
	"image"
	"image/color"
	"image/png"

	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

const totemMarkerPath = "textures/items/cinnabar_totem_marker"

// totemTexturePack replaces the ordinary totem icon with original asymmetric marker artwork.
func totemTexturePack() (*resource.Pack, error) {
	var engine resource.Version
	if _, err := fmt.Sscanf(protocol.CurrentVersion, "%d.%d.%d", &engine[0], &engine[1], &engine[2]); err != nil {
		return nil, err
	}
	version := resource.Version{1, 0, 0}
	manifest, err := json.Marshal(resource.Manifest{
		FormatVersion: 2,
		Header: resource.Header{Name: "Totem texture fixture", Description: "Original activation marker",
			UUID: uuid.NewSHA1(uuid.Nil, []byte("cinnabar-totem-texture-fixture")), Version: version,
			MinimumGameVersion: engine},
		Modules: []resource.Module{{Type: "resources", Version: version,
			UUID: uuid.NewSHA1(uuid.Nil, []byte("cinnabar-totem-texture-fixture-resources")).String()}},
	})
	if err != nil {
		return nil, err
	}
	pixels := image.NewNRGBA(image.Rect(0, 0, 32, 32))
	for y := range 32 {
		for x := range 32 {
			if x < 2 || x > 29 || y < 2 || y > 29 {
				continue
			}
			shade := color.NRGBA{R: 255, B: 255, A: 255}
			if x >= 16 {
				shade = color.NRGBA{G: 255, B: 255, A: 255}
			}
			if (y >= 6 && y <= 10) || (x >= 13 && x <= 18 && y > 10 && y <= 25) {
				shade = color.NRGBA{R: 255, G: 255, A: 255}
			}
			pixels.SetNRGBA(x, y, shade)
		}
	}
	var pngBytes bytes.Buffer
	if err := png.Encode(&pngBytes, pixels); err != nil {
		return nil, err
	}
	var archive bytes.Buffer
	writer := zip.NewWriter(&archive)
	files := []struct {
		name string
		data []byte
	}{
		{name: "manifest.json", data: manifest},
		{name: "textures/item_texture.json", data: []byte(fmt.Sprintf(
			`{"resource_pack_name":"totem_fixture","texture_name":"atlas.items","texture_data":{"totem":{"textures":%q}}}`, totemMarkerPath))},
		{name: totemMarkerPath + ".png", data: pngBytes.Bytes()},
	}
	for _, entry := range files {
		file, err := writer.Create(entry.name)
		if err != nil {
			return nil, err
		}
		if _, err := file.Write(entry.data); err != nil {
			return nil, err
		}
	}
	if err := writer.Close(); err != nil {
		return nil, err
	}
	return resource.ReadBytes(archive.Bytes())
}
