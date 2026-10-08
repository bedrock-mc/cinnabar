package main

import (
	"encoding/json"
	"fmt"
)

const actorBurstComplexBones = 32
const actorBurstCubesPerBone = 8

// actorBurstComplexGeometry adds original fine details to stress unique source and mesh preparation.
func actorBurstComplexGeometry(variant int) []byte {
	var document map[string]any
	if err := json.Unmarshal(actorBurstGeometry(variant), &document); err != nil {
		panic(fmt.Errorf("decode generated actor geometry: %w", err))
	}
	model := document["minecraft:geometry"].([]any)[0].(map[string]any)
	bones := model["bones"].([]any)
	for bone := range actorBurstComplexBones {
		cubes := make([]any, 0, actorBurstCubesPerBone)
		for cube := range actorBurstCubesPerBone {
			index := bone*actorBurstCubesPerBone + cube
			cubes = append(cubes, map[string]any{
				"origin": []float64{-3.5 + float64(index%8)*0.9, 12.2 + float64(index/8%16)*0.6, 2.05 + float64(index/128)*0.4},
				"size":   []float64{0.3, 0.3, 0.3}, "uv": []int{16, 16},
			})
		}
		bones = append(bones, map[string]any{
			"name": fmt.Sprintf("fixtureDetail%d", bone), "parent": "body",
			"pivot": []int{0, 0, 0}, "cubes": cubes,
		})
	}
	model["bones"] = bones
	encoded, err := json.Marshal(document)
	if err != nil {
		panic(fmt.Errorf("encode generated actor geometry: %w", err))
	}
	return encoded
}
