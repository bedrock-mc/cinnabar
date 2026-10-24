package main

import (
	"image"
	"image/color"
	"math/rand/v2"

	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/block/customblock"
	"github.com/df-mc/dragonfly/server/block/model"
	"github.com/df-mc/dragonfly/server/cmd"
	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/item/category"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
)

const (
	luckyBlockID      = "fixture:lucky_block"
	luckyBlockTexture = "fixture_lucky_block"
	// luckyBlockHardness gives a short, visibly cracking survival break by hand.
	luckyBlockHardness = 0.4
)

// luckyBlockHash is the block's base hash, taken once at registration.
var luckyBlockHash uint64

// luckyBlock is a server-defined full cube that drops a random reward, like minigame lucky blocks.
type luckyBlock struct{}

var (
	_ world.CustomBlockBuildable = luckyBlock{}
	_ world.CustomItem           = luckyBlock{}
	_ block.Breakable            = luckyBlock{}
)

// registerLuckyBlock adds the block and its item; it must run before the server builds its worlds.
func registerLuckyBlock() {
	luckyBlockHash = block.NextHash()
	world.RegisterBlock(luckyBlock{})
	world.RegisterItem(luckyBlock{})
	cmd.Register(cmd.New("lucky", "Places lucky blocks in front of you", nil, luckyPlace{}))
}

func (luckyBlock) EncodeBlock() (string, map[string]any) { return luckyBlockID, nil }
func (luckyBlock) Hash() (uint64, uint64)                { return luckyBlockHash, 0 }
func (luckyBlock) Model() world.BlockModel               { return model.Solid{} }
func (luckyBlock) EncodeItem() (string, int16)           { return luckyBlockID, 0 }
func (luckyBlock) Name() string                          { return "Lucky Block" }
func (luckyBlock) Geometry() []byte                      { return nil }
func (luckyBlock) Category() category.Category           { return category.Construction() }
func (luckyBlock) Texture() image.Image                  { return luckyBlockImage() }

func (luckyBlock) Textures() map[string]image.Image {
	return map[string]image.Image{luckyBlockTexture: luckyBlockImage()}
}

func (luckyBlock) Properties() customblock.Properties {
	full := cube.Box(0, 0, 0, 1, 1, 1)
	return customblock.Properties{
		Cube:         true,
		CollisionBox: full,
		SelectionBox: full,
		Textures: map[string]customblock.Material{
			"*": customblock.NewMaterial(luckyBlockTexture, customblock.OpaqueRenderMethod()),
		},
	}
}

// BreakInfo drops one random reward instead of the block itself.
func (luckyBlock) BreakInfo() block.BreakInfo {
	rewards := []world.Item{item.GoldIngot{}, item.Diamond{}, item.Apple{}, item.Emerald{}}
	return block.BreakInfo{
		Hardness:        luckyBlockHardness,
		BlastResistance: luckyBlockHardness,
		Harvestable:     func(item.Tool) bool { return true },
		Effective:       func(item.Tool) bool { return false },
		Drops: func(item.Tool, []item.Enchantment) []item.Stack {
			return []item.Stack{item.NewStack(rewards[rand.IntN(len(rewards))], 1)}
		},
	}
}

// luckyBlockImage is original yellow artwork with a dark question-mark-like stripe.
func luckyBlockImage() image.Image {
	pixels := image.NewNRGBA(image.Rect(0, 0, 16, 16))
	for y := range 16 {
		for x := range 16 {
			shade := color.NRGBA{R: 250, G: 200, B: 30, A: 255}
			if x == 0 || y == 0 || x == 15 || y == 15 {
				shade = color.NRGBA{R: 180, G: 120, B: 10, A: 255}
			} else if (y == 4 && x >= 6 && x <= 9) || (x == 10 && y >= 5 && y <= 7) ||
				(y == 8 && x >= 7 && x <= 9) || (x == 7 && y == 9) || (x == 7 && y == 11) {
				shade = color.NRGBA{R: 90, G: 50, B: 0, A: 255}
			}
			pixels.SetNRGBA(x, y, shade)
		}
	}
	return pixels
}

// luckyPlace puts three lucky blocks two to four blocks ahead of the player at foot level.
type luckyPlace struct{}

func (luckyPlace) Run(src cmd.Source, o *cmd.Output, tx *world.Tx) {
	p, ok := src.(*player.Player)
	if !ok {
		o.Error("only players can place lucky blocks")
		return
	}
	direction := p.Rotation().Direction().Face()
	origin := cube.PosFromVec3(p.Position())
	for distance := 2; distance <= 4; distance++ {
		pos := origin
		for range distance {
			pos = pos.Side(direction)
		}
		tx.SetBlock(pos, luckyBlock{}, nil)
	}
	o.Printf("Placed lucky blocks facing %v", direction)
}
