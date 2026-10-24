package main

import (
	_ "embed"
	"encoding/json"
)

// entityGalleryPaintingData is generated with `cargo run -p assets --example export_painting_motives`.
//
//go:embed painting_motives.json
var entityGalleryPaintingData []byte

type entityGalleryPaintingMotive struct {
	Title string     `json:"title"`
	Size  [2]float32 `json:"size"`
}

var entityGalleryPaintingMotives = loadEntityGalleryPaintingMotives()

// loadEntityGalleryPaintingMotives reads the embedded copy checked against the client's catalog.
func loadEntityGalleryPaintingMotives() []entityGalleryPaintingMotive {
	var motives []entityGalleryPaintingMotive
	if err := json.Unmarshal(entityGalleryPaintingData, &motives); err != nil {
		panic(err)
	}
	return motives
}

// entityGalleryPaintingHeight returns the rendered height in blocks, including the client's fallback.
func entityGalleryPaintingHeight(title string) float32 {
	for _, motive := range entityGalleryPaintingMotives {
		if motive.Title == title {
			return motive.Size[1] / 16
		}
	}
	return entityGalleryPaintingMotives[0].Size[1] / 16
}
