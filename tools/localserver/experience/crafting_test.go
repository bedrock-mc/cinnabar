package experience

import (
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
)

// The probe's 0.6 behaviors.
const (
	// probeCraft asks the server's recipes what two planks in a column make.
	probeCraft = 28
	// probeSchedule schedules an update of its block two ticks on.
	probeSchedule = 29
)

// The server's recipes reach the guest: two oak planks in a column make four sticks, through the
// planks item tag.
func TestServerRecipesAnswerTheGuestsGrid(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	// As the server does: its recipes register once its blocks are final, and the helper loads
	// again with them.
	vanillaRecipes()
	if err := f.host.Reload("probe"); err != nil {
		t.Fatalf("Reload: %v", err)
	}
	pos := probePos(probeCraft)
	f.place(probeCounter, pos, nil)
	f.activate(pos)
	f.waitTells(5*time.Second, "crafted minecraft:stick 4 short none")
}

// A scheduled update commits, and when it is due the guest runs on its block: the probe counts
// the update in the block's data.
func TestScheduledUpdateRunsOnItsBlock(t *testing.T) {
	log, logs := testLog(t)
	sup, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	pos := probePos(probeSchedule)
	f.place(probeCounter, pos, nil)
	f.activate(pos)
	f.waitTells(5*time.Second, "schedule ok zero denied foreign not-owned")
	// A world ticks, and so runs scheduled updates, only while something views it.
	f.do(func(tx *world.Tx) {
		viewer := world.NewLoader(1, tx.World(), world.NopViewer{})
		viewer.Move(tx, mgl64.Vec3{float64(pos.X), float64(pos.Y), float64(pos.Z)})
		viewer.Load(tx, 9)
	})
	// The probe logs the count it wrote once the update's result committed.
	waitForRecord(t, logs, func(r map[string]any) bool {
		line, _ := r["line"].(string)
		return strings.Contains(line, "scheduled count 1")
	})
	f.waitData("probe", pos, counter(1))
}

// Every crafting-table recipe the server has fits the protocol's bounds, and the load request that
// carries them, with the server's items, fits a frame.
func TestServerRecipesFitTheLoadFrame(t *testing.T) {
	registered(t)
	vanillaRecipes()
	recipes := serverRecipes()
	sticks := slices.IndexFunc(recipes, func(r Recipe) bool {
		return r.Output.ID == "minecraft:stick" && slices.ContainsFunc(r.Inputs[0], func(m ItemMatch) bool {
			return m.ID == "minecraft:oak_planks" && m.Metadata == nil
		})
	})
	if sticks < 0 {
		t.Fatal("no recipe makes sticks of oak planks")
	}
	if r := recipes[sticks]; r.Shape == nil || *r.Shape != [2]uint8{1, 2} || r.Output.Count != 4 {
		t.Fatalf("the stick recipe is %+v", r)
	}
	for _, r := range recipes {
		for _, input := range r.Inputs {
			if len(input) > maxRecipeInputItems {
				t.Fatalf("%s takes an input of %d items", r.Output.ID, len(input))
			}
		}
	}
	load := Request{Load: &LoadRequest{Dir: "/srv/experiences/probe", Items: serverItems(), Recipes: recipes}}
	if _, err := encodeFrame(load); err != nil {
		t.Fatalf("the load request: %v", err)
	}
}
