package main

import (
	"log/slog"
	"slices"
	"testing"

	"github.com/cinnabar-mc/cinnabar/tools/localserver/experience"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
)

// entityHookRecorder retains identities observed through the real player input methods.
type entityHookRecorder struct {
	actor, target *world.EntityHandle
	attacks       []bool
}

// EntityInteraction records the input's actor, target and attack classification.
func (h *entityHookRecorder) EntityInteraction(p *player.Player, target world.Entity, attacked bool) {
	h.actor, h.target = p.H(), target.H()
	h.attacks = append(h.attacks, attacked)
}

// TeleportPlayer leaves unrelated native movement unchanged in this fixture.
func (*entityHookRecorder) TeleportPlayer(*player.Player, experience.Position) bool { return false }

// PlayerDisconnected has no persistent state to retire in this fixture.
func (*entityHookRecorder) PlayerDisconnected(*player.Player) {}

func TestLocalEntityInputUsesExperienceHooks(t *testing.T) {
	for _, installed := range []bool{false, true} {
		t.Run(map[bool]string{false: "without_host", true: "with_host"}[installed], func(t *testing.T) {
			w := world.Config{Synchronous: true, Log: slog.New(slog.DiscardHandler)}.New()
			defer w.Close()
			actor := world.EntitySpawnOpts{Position: mgl64.Vec3{0, 70, 0}}.New(player.Type, player.Config{Name: "Actor"})
			target := world.EntitySpawnOpts{Position: mgl64.Vec3{0, 70, 1}}.New(player.Type, player.Config{Name: "Target"})
			recorder := &entityHookRecorder{}
			if err := w.Do(func(tx *world.Tx) {
				p := tx.AddEntity(actor).(*player.Player)
				e := tx.AddEntity(target)
				handler := &localPlayerHandler{}
				if installed {
					handler.host = recorder
				}
				p.Handle(handler)
				if !p.UseItemOnEntity(e) {
					t.Error("in-range interaction was refused")
				}
				p.AttackEntity(e)
				tx.RemoveEntity(e)
				tx.RemoveEntity(p)
				_ = target.Close()
				_ = actor.Close()
			}).Wait(t.Context()); err != nil {
				t.Fatal(err)
			}
			if installed {
				if !slices.Equal(recorder.attacks, []bool{false, true}) || recorder.actor != actor || recorder.target != target {
					t.Fatalf("entity input did not reach the installed host: %+v", recorder)
				}
			} else if len(recorder.attacks) != 0 {
				t.Fatal("input reached an uninstalled host")
			}
		})
	}
}
