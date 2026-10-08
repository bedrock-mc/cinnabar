package main

import (
	"fmt"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
)

const actorBurstSkinSide = 64
const actorBurstGeometryName = "geometry.cinnabar.actor_burst"

// actorBurstSkin creates original striped texels and an original articulated cube model.
func actorBurstSkin(variant, model int) protocol.Skin {
	pixels := make([]byte, actorBurstSkinSide*actorBurstSkinSide*4)
	for y := range actorBurstSkinSide {
		for x := range actorBurstSkinSide {
			at := (y*actorBurstSkinSide + x) * 4
			pixels[at], pixels[at+1], pixels[at+2], pixels[at+3] = byte(variant), byte(64+(x/8)*20), byte(64+(y/8)*20), 255
		}
	}
	return protocol.Skin{
		SkinID:         fmt.Sprintf("cinnabar-fixture-%d-%d", variant, model),
		SkinImageWidth: actorBurstSkinSide, SkinImageHeight: actorBurstSkinSide, SkinData: pixels,
		SkinResourcePatch: []byte(fmt.Sprintf(`{"geometry":{"default":%q}}`, actorBurstGeometryName)),
		SkinGeometry:      actorBurstGeometry(model), GeometryDataEngineVersion: []byte("1.12.0"),
		ArmSize: protocol.ArmSizeWide, Trusted: true,
	}
}

// actorBurstGeometry varies the torso width to exercise distinct immutable geometry preparation.
func actorBurstGeometry(variant int) []byte {
	width := 8 + float64(variant)/512
	return []byte(fmt.Sprintf(`{"format_version":"1.12.0","minecraft:geometry":[{
"description":{"identifier":%q,"texture_width":%d,"texture_height":%d},
"bones":[
{"name":"root","pivot":[0,0,0]},
{"name":"body","parent":"root","pivot":[0,24,0],"cubes":[{"origin":[-4,12,-2],"size":[%g,12,4],"uv":[16,16]}]},
{"name":"head","parent":"body","pivot":[0,24,0],"cubes":[{"origin":[-4,24,-4],"size":[8,8,8],"uv":[0,0]}]},
{"name":"leftArm","parent":"body","pivot":[5,22,0],"cubes":[{"origin":[4,12,-2],"size":[4,12,4],"uv":[32,48]}]},
{"name":"rightArm","parent":"body","pivot":[-5,22,0],"cubes":[{"origin":[-8,12,-2],"size":[4,12,4],"uv":[40,16]}]},
{"name":"leftLeg","parent":"root","pivot":[2,12,0],"cubes":[{"origin":[0,0,-2],"size":[4,12,4],"uv":[16,48]}]},
{"name":"rightLeg","parent":"root","pivot":[-2,12,0],"cubes":[{"origin":[-4,0,-2],"size":[4,12,4],"uv":[0,16]}]}
]}]}`, actorBurstGeometryName, actorBurstSkinSide, actorBurstSkinSide, width))
}
