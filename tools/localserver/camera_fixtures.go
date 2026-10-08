package main

import (
	"bytes"

	"github.com/go-gl/mathgl/mgl32"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

const cameraFixtureName = "cinnabar:camera_test"
const cameraFixtureActor = 9000001
const cameraFixtureDuration float32 = 6

// cameraFixturePackets covers named and inline paths, aim registries and clearing both systems.
func cameraFixturePackets(command string, origin mgl32.Vec3) []packet.Packet {
	switch command {
	case "spline", "inline":
		points := []mgl32.Vec3{origin.Add(mgl32.Vec3{-4, 3, -4}), origin.Add(mgl32.Vec3{0, 5, -4}), origin.Add(mgl32.Vec3{4, 3, -4}), origin.Add(mgl32.Vec3{0, 3, -8})}
		return []packet.Packet{
			&packet.CameraPresets{Presets: []protocol.CameraPreset{{Name: cameraFixtureName, Parent: "minecraft:free"}}},
			cameraSplineRegistryFixture(points),
			&packet.CameraInstruction{Set: protocol.Option(protocol.CameraInstructionSet{Preset: 0, Position: protocol.Option(origin)})},
			cameraSplineInstructionFixture(points, command == "spline"),
		}
	case "aim":
		return []packet.Packet{
			&packet.CameraAimAssistPresets{
				Categories: []protocol.CameraAimAssistCategory{{Name: "targets", Priorities: protocol.CameraAimAssistPriorities{EntityDefault: protocol.Option(int32(100)), BlockDefault: protocol.Option(int32(0))}}},
				Presets:    []protocol.CameraAimAssistPreset{{Identifier: cameraFixtureName, HandSettings: protocol.Option("targets"), DefaultItemSettings: protocol.Option("targets")}},
			},
			&packet.CameraAimAssistActorPriority{PriorityData: []protocol.CameraAimAssistActorPriorityData{{Priority: 100}}},
			&packet.AddActor{EntityUniqueID: cameraFixtureActor, EntityRuntimeID: cameraFixtureActor, EntityType: "minecraft:pig", Position: origin.Add(mgl32.Vec3{1, -1.6, 5}), EntityMetadata: protocol.EntityMetadata{
				protocol.EntityDataKeyWidth: float32(0.9), protocol.EntityDataKeyHeight: float32(0.9), protocol.EntityDataKeyAimAssistPriorityActorID: int32(0),
			}},
			&packet.CameraAimAssist{Preset: cameraFixtureName, Angle: mgl32.Vec2{60, 60}, Distance: 12, TargetMode: protocol.AimAssistTargetModeAngle},
		}
	case "clear":
		return []packet.Packet{&packet.CameraInstruction{Clear: protocol.Option(true)}, &packet.CameraAimAssist{Action: packet.CameraAimAssistActionClear}, &packet.RemoveActor{EntityUniqueID: cameraFixtureActor}}
	}
	return nil
}

// cameraSplineRegistryFixture writes the camera registry's direct string and optional easing fields.
func cameraSplineRegistryFixture(points []mgl32.Vec3) packet.Packet {
	var body bytes.Buffer
	w := protocol.NewWriter(&body, 0)
	count, name, duration, kind := uint32(1), cameraFixtureName, cameraFixtureDuration, "catmullrom"
	w.Varuint32(&count)
	w.String(&name)
	w.Float32(&duration)
	w.String(&kind)
	protocol.FuncSlice(w, &points, w.Vec3)
	cameraFixtureTracks(w, true)
	return &packet.Unknown{PacketID: packet.IDCameraSpline, Payload: body.Bytes()}
}

// cameraSplineInstructionFixture writes the direct curve selector, identifier and load flag.
func cameraSplineInstructionFixture(points []mgl32.Vec3, named bool) packet.Packet {
	var body bytes.Buffer
	w := protocol.NewWriter(&body, 0)
	absent, present := false, true
	for i := 0; i < 6; i++ {
		w.Bool(&absent)
	}
	w.Bool(&present)
	duration, kind := cameraFixtureDuration, uint8(0)
	w.Float32(&duration)
	w.Uint8(&kind)
	protocol.FuncSlice(w, &points, w.Vec3)
	cameraFixtureTracks(w, false)
	name := cameraFixtureName
	w.String(&name)
	w.Bool(&named)
	w.Bool(&absent)
	w.Bool(&absent)
	return &packet.Unknown{PacketID: packet.IDCameraInstruction, Payload: body.Bytes()}
}

// cameraFixtureTracks drives the same progress and Euler rotation through both packet layouts.
func cameraFixtureTracks(w protocol.IO, optionalEase bool) {
	count := uint32(2)
	w.Varuint32(&count)
	for _, frame := range [][2]float32{{0, 0}, {1, cameraFixtureDuration}} {
		value, time, easing := frame[0], frame[1], "linear"
		w.Float32(&value)
		w.Float32(&time)
		if optionalEase {
			present := true
			w.Bool(&present)
		}
		w.String(&easing)
	}
	count = 2
	w.Varuint32(&count)
	for _, frame := range []struct {
		rotation mgl32.Vec3
		time     float32
	}{{mgl32.Vec3{-35, 180, 0}, 0}, {mgl32.Vec3{-35, 200, 10}, cameraFixtureDuration}} {
		easing := "linear"
		w.Vec3(&frame.rotation)
		w.Float32(&frame.time)
		if optionalEase {
			present := true
			w.Bool(&present)
		}
		w.String(&easing)
	}
}
