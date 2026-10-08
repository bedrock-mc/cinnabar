package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"reflect"
	"testing"

	"github.com/go-gl/mathgl/mgl32"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestActorBurstPacketsRoundtripAndShareAppearanceAsRequested(t *testing.T) {
	for _, mode := range []string{"shared", "skins", "geometry", "complex"} {
		t.Run(mode, func(t *testing.T) {
			origin := mgl32.Vec3{20, 65, -10}
			packets := actorBurstPackets(4, mode, origin)
			if !reflect.DeepEqual(packets, actorBurstPackets(4, mode, origin)) {
				t.Fatal("fixture is not deterministic")
			}
			var buffer bytes.Buffer
			packets[0].Marshal(protocol.NewWriter(&buffer, 0))
			var list packet.PlayerList
			list.Marshal(protocol.NewReader(&buffer, 0, true))
			if len(list.Entries) != 4 || len(packets) != 5 {
				t.Fatal("incorrect crowd count")
			}
			for index, entry := range list.Entries {
				if entry.ActionType != protocol.PlayerListActionAdd || entry.XUID != "" || !json.Valid(entry.Skin.SkinGeometry) {
					t.Fatal("invalid synthetic player appearance")
				}
				buffer.Reset()
				packets[index+1].Marshal(protocol.NewWriter(&buffer, 0))
				var actor packet.AddPlayer
				actor.Marshal(protocol.NewReader(&buffer, 0, true))
				if actor.UUID != entry.UUID || actor.AbilityData.EntityUniqueID != entry.EntityUniqueID || actor.EntityRuntimeID != uint64(entry.EntityUniqueID) {
					t.Fatal("list and actor identities differ")
				}
				if actor.Position[2] <= origin[2] || actor.Position[1] != origin[1] {
					t.Fatal("crowd does not follow the requested origin")
				}
				if index > 0 {
					sameSkin := bytes.Equal(entry.Skin.SkinData, list.Entries[0].Skin.SkinData)
					sameModel := bytes.Equal(entry.Skin.SkinGeometry, list.Entries[0].Skin.SkinGeometry)
					if sameSkin != (mode == "shared") || sameModel != (mode != "geometry" && mode != "complex") {
						t.Fatal("mode did not select independent skin/model variation")
					}
				}
			}
			removed := actorBurstClear(4, mode)
			for index := range 4 {
				if removed[index].(*packet.RemoveActor).EntityUniqueID != list.Entries[index].EntityUniqueID {
					t.Fatal("clear removed another actor")
				}
				entry := removed[4].(*packet.PlayerList).Entries[index]
				if entry.ActionType != protocol.PlayerListActionRemove || entry.UUID != list.Entries[index].UUID {
					t.Fatal("clear left a player-list entry")
				}
			}
		})
	}
}

func TestActorBurstRejectsInvalidCommandsAndBoundsTheCrowd(t *testing.T) {
	for _, command := range []string{"/actorburst", "/actorburst 0 shared", fmt.Sprintf("/actorburst %d shared", actorBurstLimit+1), "/actorburst -1 skins", "/actorburst 2 unknown", "/actorburst 2", "/actorburst 2 skins extra", "/other 2 shared"} {
		if _, _, ok := actorBurstCommand(command); ok {
			t.Fatalf("accepted %q", command)
		}
	}
	if n, mode, ok := actorBurstCommand(fmt.Sprintf("/actorburst %d geometry", actorBurstLimit)); !ok || n != actorBurstLimit || mode != "geometry" {
		t.Fatal("maximum burst was rejected")
	}
	if n, _, ok := actorBurstCommand("/actorburst clear"); !ok || n != 0 {
		t.Fatal("clear was rejected")
	}
	settings, err := parseSettings([]string{"-dir", t.TempDir(), "-addr", "127.0.0.1:19132", "-actor-burst"}, io.Discard)
	if err != nil || !settings.actorBurst {
		t.Fatalf("fixture flag: %v", err)
	}
}

func TestActorBurstReplacementClearsOnlyItsPreviousCrowd(t *testing.T) {
	movement := &packet.PlayerAuthInput{Position: mgl32.Vec3{4, 70, 8}}
	ordinary := &packet.CommandRequest{CommandLine: "/give stone"}
	inner := &cameraFixtureConn{read: []packet.Packet{
		movement,
		&packet.CommandRequest{CommandLine: "/actorburst 2 shared"},
		&packet.CommandRequest{CommandLine: "/actorburst 3 skins"},
		&packet.CommandRequest{CommandLine: "/actorburst clear"},
		ordinary,
	}}
	conn := &actorBurstConn{Conn: inner}
	if got, err := conn.ReadPacket(); err != nil || got != movement {
		t.Fatal("movement was swallowed")
	}
	if got, err := conn.ReadPacket(); err != nil || got != ordinary {
		t.Fatal("ordinary command was swallowed")
	}
	if len(inner.written) != 14 || conn.count != 0 {
		t.Fatal("replacement or clear left stale actors")
	}
	if _, ok := inner.written[3].(*packet.RemoveActor); !ok {
		t.Fatal("replacement did not remove the old crowd first")
	}
	if _, ok := inner.written[6].(*packet.PlayerList); !ok {
		t.Fatal("replacement list did not follow removals")
	}
	if inner.written[1].(*packet.AddPlayer).Position[1] != movement.Position[1] {
		t.Fatal("burst used a stale origin")
	}
}

func TestActorBurstComplexGeometryAddsBoundedOriginalDetails(t *testing.T) {
	var base, complex struct {
		Geometry []struct {
			Bones []struct {
				Cubes []json.RawMessage `json:"cubes"`
			} `json:"bones"`
		} `json:"minecraft:geometry"`
	}
	if err := json.Unmarshal(actorBurstGeometry(0), &base); err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(actorBurstComplexGeometry(0), &complex); err != nil {
		t.Fatal(err)
	}
	original := len(base.Geometry[0].Bones)
	if len(complex.Geometry[0].Bones) != original+actorBurstComplexBones {
		t.Fatal("complex model lost its original bones or requested details")
	}
	for _, bone := range complex.Geometry[0].Bones[original:] {
		if len(bone.Cubes) != actorBurstCubesPerBone {
			t.Fatal("detail cube count changed")
		}
	}
	if count, mode, ok := actorBurstCommand("/actorburst 128 complex"); !ok || count != 128 || mode != "complex" {
		t.Fatal("complex source command was rejected")
	}
}
