package localserver

import (
	"fmt"
	"math"

	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl32"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// actorBurstSwingPeriod repeats the swing schedule every this many client input ticks.
const actorBurstSwingPeriod = 40

// actorBurstSwingTicks are the "swing" schedule's swing ticks: two lone swings, then one every
// other tick, as fast clicking sends them.
var actorBurstSwingTicks = [...]int{0, 10, 20, 22, 24, 26, 28}

// actorBurstStrafeBlocks is how far "strafe" fixtures sway sideways each way over one period.
const actorBurstStrafeBlocks = 1.5

// actorBurstEyeHeight converts between the feet AddPlayer places and the eye that input and
// movement packets carry.
const actorBurstEyeHeight = 1.62

// actorBurstSwingMode reports whether mode is a sword fixture that swings every input tick.
func actorBurstSwingMode(mode string) bool {
	return mode == "swing" || mode == "strafe"
}

// actorBurstSwingPosition stands sword fixtures' feet in a row three blocks ahead of the eye.
func actorBurstSwingPosition(index int, origin mgl32.Vec3) mgl32.Vec3 {
	return origin.Add(mgl32.Vec3{float32(index) * 1.5, -actorBurstEyeHeight, 3})
}

// actorBurstSwordPackets put a diamond sword in every fixture's main hand.
func actorBurstSwordPackets(count int) []packet.Packet {
	runtimeID, metadata, ok := world.ItemRuntimeID(item.Sword{Tier: item.ToolTierDiamond})
	if !ok {
		return nil
	}
	stack := protocol.ItemInstance{Stack: protocol.ItemStack{
		ItemType: protocol.ItemType{NetworkID: runtimeID, MetadataValue: uint32(metadata)},
		Count:    1, NBTData: map[string]any{}, CanBePlacedOn: []string{}, CanBreak: []string{},
	}}
	packets := make([]packet.Packet, count)
	for index := range count {
		packets[index] = &packet.MobEquipment{
			EntityRuntimeID: uint64(actorBurstFirstID + index), NewItem: stack, WindowID: protocol.WindowIDInventory,
		}
	}
	return packets
}

// actorBurstSwingTick returns input tick `tick`'s fixture packets for a crowd spawned at the
// eye `origin`: "strafe" fixtures sway sideways facing the origin and swing every other tick
// (10 clicks a second); "swing" fixtures stand still on actorBurstSwingTicks.
func actorBurstSwingTick(mode string, count, tick int, origin mgl32.Vec3) []packet.Packet {
	phase := tick % actorBurstSwingPeriod
	swings := false
	var packets []packet.Packet
	switch mode {
	case "strafe":
		swings = tick%2 == 0
		sway := float32(actorBurstStrafeBlocks * math.Sin(2*math.Pi*float64(phase)/actorBurstSwingPeriod))
		for index := range count {
			feet := actorBurstSwingPosition(index, origin).Add(mgl32.Vec3{sway, 0, 0})
			toward := origin.Sub(feet)
			yaw := float32(math.Atan2(float64(-toward[0]), float64(toward[2])) * 180 / math.Pi)
			packets = append(packets, &packet.MoveActorAbsolute{
				EntityRuntimeID: uint64(actorBurstFirstID + index),
				Position:        feet.Add(mgl32.Vec3{0, actorBurstEyeHeight, 0}),
				Rotation:        mgl32.Vec3{0, yaw, yaw},
				Flags:           packet.MoveFlagOnGround,
			})
		}
	case "swing":
		for _, scheduled := range actorBurstSwingTicks {
			swings = swings || phase == scheduled
		}
	}
	if !swings {
		return packets
	}
	for index := range count {
		packets = append(packets, &packet.Animate{
			ActionType: packet.AnimateActionSwingArm, EntityRuntimeID: uint64(actorBurstFirstID + index),
			SwingSource: packet.AnimateSwingSourceAttack,
		})
	}
	return packets
}

// actorBurstHumanoidGeometry is an original model with the standard player skin's bone
// hierarchy (waist, sleeves, item and cape bones), so fixtures pose like real players.
func actorBurstHumanoidGeometry() []byte {
	return []byte(fmt.Sprintf(`{"format_version":"1.12.0","minecraft:geometry":[{
"description":{"identifier":%q,"texture_width":%d,"texture_height":%d},
"bones":[
{"name":"root","pivot":[0,0,0]},
{"name":"waist","parent":"root","pivot":[0,12,0]},
{"name":"body","parent":"waist","pivot":[0,24,0],"cubes":[{"origin":[-4,12,-2],"size":[8,12,4],"uv":[16,16]}]},
{"name":"jacket","parent":"body","pivot":[0,24,0],"cubes":[{"origin":[-4,12,-2],"size":[8,12,4],"uv":[16,32],"inflate":0.25}]},
{"name":"cape","parent":"body","pivot":[0,24,3]},
{"name":"head","parent":"body","pivot":[0,24,0],"cubes":[{"origin":[-4,24,-4],"size":[8,8,8],"uv":[0,0]}]},
{"name":"hat","parent":"head","pivot":[0,24,0],"cubes":[{"origin":[-4,24,-4],"size":[8,8,8],"uv":[32,0],"inflate":0.5}]},
{"name":"leftArm","parent":"body","pivot":[5,22,0],"cubes":[{"origin":[4,12,-2],"size":[4,12,4],"uv":[32,48]}]},
{"name":"leftSleeve","parent":"leftArm","pivot":[5,22,0],"cubes":[{"origin":[4,12,-2],"size":[4,12,4],"uv":[48,48],"inflate":0.25}]},
{"name":"leftItem","parent":"leftArm","pivot":[6,15,1]},
{"name":"rightArm","parent":"body","pivot":[-5,22,0],"cubes":[{"origin":[-8,12,-2],"size":[4,12,4],"uv":[40,16]}]},
{"name":"rightSleeve","parent":"rightArm","pivot":[-5,22,0],"cubes":[{"origin":[-8,12,-2],"size":[4,12,4],"uv":[40,32],"inflate":0.25}]},
{"name":"rightItem","parent":"rightArm","pivot":[-6,15,1]},
{"name":"leftLeg","parent":"root","pivot":[1.9,12,0],"cubes":[{"origin":[-0.1,0,-2],"size":[4,12,4],"uv":[16,48]}]},
{"name":"leftPants","parent":"leftLeg","pivot":[1.9,12,0],"cubes":[{"origin":[-0.1,0,-2],"size":[4,12,4],"uv":[0,48],"inflate":0.25}]},
{"name":"rightLeg","parent":"root","pivot":[-1.9,12,0],"cubes":[{"origin":[-3.9,0,-2],"size":[4,12,4],"uv":[0,16]}]},
{"name":"rightPants","parent":"rightLeg","pivot":[-1.9,12,0],"cubes":[{"origin":[-3.9,0,-2],"size":[4,12,4],"uv":[0,32],"inflate":0.25}]}
]}]}`, actorBurstGeometryName, actorBurstSkinSide, actorBurstSkinSide))
}
