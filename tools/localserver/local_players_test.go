package localserver

import (
	"errors"
	"io"
	"log/slog"
	"path/filepath"
	"testing"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/item/inventory"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/player/playerdb"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/df-mc/goleveldb/leveldb"
	"github.com/go-gl/mathgl/mgl64"
	"github.com/google/uuid"
)

func TestOpaqueOverdrawKeepsPlayerProviderFallback(t *testing.T) {
	cfg := settings{
		dir: t.TempDir(), addr: "127.0.0.1:0",
		gameMode: "creative", diff: "normal", opaqueOverdraw: true,
	}
	conf, err := cfg.userConfig().Config(slog.New(slog.NewTextHandler(io.Discard, nil)))
	if err != nil {
		t.Fatal(err)
	}
	cfg.configureLocalPlayers(&conf)
	if conf.PlayerProvider != nil {
		t.Fatalf("disabled player storage must keep Dragonfly's no-op fallback, got %T", conf.PlayerProvider)
	}
}

func TestEditedGameModeRestoresHostAndRoundTripsPlayerData(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "players")
	w := world.Config{}.New()
	defer w.Close()
	lookup := func(world.Dimension) *world.World { return w }
	id := uuid.New()
	data := player.Config{
		UUID: id, Name: "Host", GameMode: world.GameModeSurvival,
		Position: mgl64.Vec3{2, 70, -5}, Health: 15, Experience: 42,
		Inventory: inventory.New(36, nil), OffHand: inventory.New(1, nil),
		Armour: inventory.NewArmour(nil), EnderChestInventory: inventory.New(27, nil),
	}
	if err := data.Inventory.SetItem(3, item.NewStack(block.Dirt{}, 7)); err != nil {
		t.Fatal(err)
	}
	provider, err := playerdb.NewProvider(dir)
	if err != nil {
		t.Fatal(err)
	}
	if err := provider.Save(id, data, w); err != nil {
		t.Fatal(err)
	}
	if err := provider.Close(); err != nil {
		t.Fatal(err)
	}

	for _, mode := range []string{"creative", "adventure", "survival"} {
		t.Run(mode, func(t *testing.T) {
			provider, err := playerdb.NewProvider(dir)
			if err != nil {
				t.Fatal(err)
			}
			defer provider.Close()
			cfg := settings{gameMode: mode, diff: "hard"}
			cfg.applyTo(w)
			conf := server.Config{PlayerProvider: provider}
			cfg.configureLocalPlayers(&conf)
			loaded, loadedWorld, err := conf.PlayerProvider.Load(id, lookup)
			if err != nil {
				t.Fatal(err)
			}
			want, _ := cfg.worldGameMode()
			if loaded.GameMode != want || w.DefaultGameMode() != want {
				t.Fatalf("host mode = %v, default = %v, want %v", loaded.GameMode, w.DefaultGameMode(), want)
			}
			stack, _ := loaded.Inventory.Item(3)
			if loadedWorld != w || loaded.Position != data.Position || loaded.Health != data.Health || loaded.Experience != data.Experience || stack.Count() != 7 {
				t.Fatalf("restored host lost saved data: %+v, stack = %v", loaded, stack)
			}
			if err := conf.PlayerProvider.Save(id, loaded, loadedWorld); err != nil {
				t.Fatal(err)
			}
			saved, _, err := provider.Load(id, lookup)
			if err != nil || saved.GameMode != want {
				t.Fatalf("saved mode = %v, err = %v, want %v", saved.GameMode, err, want)
			}
		})
	}
}

func TestNewLocalPlayerKeepsWorldDefault(t *testing.T) {
	provider, err := playerdb.NewProvider(filepath.Join(t.TempDir(), "players"))
	if err != nil {
		t.Fatal(err)
	}
	conf := server.Config{PlayerProvider: provider}
	cfg := settings{gameMode: "creative", diff: "normal"}
	cfg.configureLocalPlayers(&conf)
	defer conf.PlayerProvider.Close()
	w := world.Config{}.New()
	defer w.Close()
	cfg.applyTo(w)
	data, loadedWorld, err := conf.PlayerProvider.Load(uuid.New(), func(world.Dimension) *world.World { return w })
	if !errors.Is(err, leveldb.ErrNotFound) || loadedWorld != nil || data.GameMode != nil {
		t.Fatalf("new player must still use Dragonfly's defaults: %+v, %v, %v", data, loadedWorld, err)
	}
	if w.DefaultGameMode() != world.GameModeCreative {
		t.Fatalf("default = %v, want creative", w.DefaultGameMode())
	}
}
