package main

import (
	"bytes"
	"fmt"
	"image"
	"image/color"
	"image/png"

	"github.com/sandertv/gophertunnel/minecraft/resource"
)

const totemMarkerPath = "textures/items/cinnabar_totem_marker"

// totemTexturePack replaces the ordinary totem icon with original asymmetric marker artwork.
func totemTexturePack() (*resource.Pack, error) {
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
	return fixtureResourcePack("Totem texture fixture", "cinnabar-totem-texture-fixture", []fixtureFile{
		{"textures/item_texture.json", []byte(fmt.Sprintf(
			`{"resource_pack_name":"totem_fixture","texture_name":"atlas.items","texture_data":{"totem":{"textures":%q}}}`, totemMarkerPath))},
		{totemMarkerPath + ".png", pngBytes.Bytes()},
	})
}
