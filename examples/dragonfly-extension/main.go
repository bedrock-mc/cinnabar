// Command dragonfly-extension offers a client part and travels between two overworld worlds.
package main

import (
	"errors"
	"flag"
	"fmt"
	"log/slog"
	"os"
	"path/filepath"
	"sync"

	"github.com/cinnabar-mc/cinnabar/tools/localserver/cinnabarext"
	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/cmd"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/df-mc/dragonfly/server/world/biome"
	"github.com/df-mc/dragonfly/server/world/generator"
	"github.com/google/uuid"
)

// view is a typed world choice carried on the bundle's declared to-client channel.
type view struct {
	route cinnabarext.Route
	World uint16
}

// Route supplies the channel identity read from the signed bundle.
func (v view) Route() cinnabarext.Route { return v.route }

// Encode supplies the channel's one bounded choice field.
func (v view) Encode() (cinnabarext.Record, error) {
	if v.World >= 2 {
		return nil, errors.New("unknown world")
	}
	return cinnabarext.Record{{Choice: &v.World}}, nil
}

// application keeps only immutable player identities and the current world choice across callbacks.
type application struct {
	ext     *cinnabarext.Server
	worlds  [2]*world.World
	route   cinnabarext.Route
	current sync.Map
}

// main reports setup failures before accepting players.
func main() {
	if err := run(); err != nil {
		slog.Error("example stopped", "error", err)
		os.Exit(1)
	}
}

// run signs the offer, installs the listener, and serves two temporary worlds.
func run() error {
	keyPath := flag.String("key", "server.seed", "signing seed file")
	bundlesPath := flag.String("bundles", "bundles", "validated client bundles")
	data := flag.String("data", "server-data", "revision and resource pack directory")
	address := flag.String("address", "127.0.0.1:19132", "listen address and offer audience")
	flag.Parse()
	seed, err := os.ReadFile(*keyPath)
	if err != nil {
		return err
	}
	key, err := cinnabarext.ParseSeed(string(seed))
	if err != nil {
		return err
	}
	bundles, err := cinnabarext.ReadBundles(*bundlesPath)
	if err != nil {
		return err
	}
	if len(bundles) != 1 {
		return errors.New("this example requires exactly one sample bundle")
	}
	revision, err := cinnabarext.NextRevision(filepath.Join(*data, cinnabarext.RevisionFile))
	if err != nil {
		return err
	}
	allowed := cinnabarext.Permissions(1 << cinnabarext.PermissionMessaging)
	ext, err := cinnabarext.New(cinnabarext.Config{Key: key, Audience: *address, Revision: revision, Bundles: bundles, AllowedCapabilities: &allowed, Log: slog.Default()})
	if err != nil {
		return err
	}
	resources := filepath.Join(*data, "resources")
	if err := ext.WriteMarkerPack(resources); err != nil {
		return err
	}
	uc := exampleConfig(*address, resources)
	conf, err := uc.Config(slog.Default())
	if err != nil {
		return err
	}
	for i, listen := range conf.Listeners {
		conf.Listeners[i] = ext.Listener(listen)
	}
	srv := conf.New()
	app := &application{ext: ext}
	app.worlds = [2]*world.World{srv.World(), (world.Config{Dim: world.Overworld, Generator: generator.NewFlat(biome.Plains{}, []world.Block{block.Bedrock{}, block.Dirt{}, block.Grass{}})}).New()}
	defer app.worlds[1].Close()
	var ack cinnabarext.Route
	for _, channel := range bundles[0].Manifest.Channels {
		route := cinnabarext.Route{Bundle: bundles[0].Manifest.ID, Channel: channel.ID, Schema: channel.Schema}
		if len(channel.Fields) != 1 {
			return errors.New("sample channels require one choice field")
		}
		choice, ok := channel.Fields[0].(cinnabarext.ChoiceField)
		if !ok || choice.Variants != uint16(len(app.worlds)) {
			return errors.New("sample channel must match the world's choice count")
		}
		switch channel.Direction {
		case cinnabarext.ToClient:
			app.route = route
		case cinnabarext.ToServer:
			ack = route
		}
	}
	if app.route.Channel == "" || ack.Channel == "" {
		return errors.New("sample requires one channel each way")
	}
	if err := cinnabarext.OnMessage(ext, cinnabarext.MessageType[uint16]{Route: ack, Decode: decodeAck}, func(id uuid.UUID, world uint16) {
		slog.Info("client acknowledged world", "player", id, "world", world)
	}); err != nil {
		return err
	}
	ext.OnEpoch(func(id uuid.UUID, _ string) {
		if value, ok := app.current.Load(id); ok {
			ext.Send(id, view{route: app.route, World: value.(uint16)})
		}
	})
	cmd.Register(cmd.New("travel", "Visit the other sample world", nil, travel{app: app}))
	srv.CloseOnProgramEnd()
	srv.Listen()
	for p := range srv.Accept() {
		app.current.Store(p.UUID(), uint16(0))
		p.Handle(&handler{app: app})
		p.Message("Use /travel to visit the other overworld world.")
		ext.WorldChanged(p.UUID(), "sample:home")
	}
	return nil
}

// exampleConfig configures the sample server without world or player persistence.
func exampleConfig(address, resources string) server.UserConfig {
	uc := server.DefaultConfig()
	uc.Network.Address = address
	uc.World.SaveData = false
	uc.Players.SaveData = false
	uc.Resources.Folder = resources
	uc.Resources.Required = false
	return uc
}

// decodeAck accepts the single choice generated by the sample client.
func decodeAck(record cinnabarext.Record) (uint16, error) {
	if len(record) != 1 || record[0].Choice == nil || *record[0].Choice >= 2 {
		return 0, errors.New("invalid world acknowledgment")
	}
	return *record[0].Choice, nil
}

// travel moves a player through the source and destination world transactions.
type travel struct{ app *application }

// Run removes the player from the source before scheduling the destination transaction.
func (t travel) Run(source cmd.Source, output *cmd.Output, tx *world.Tx) {
	p, ok := source.(*player.Player)
	if !ok {
		output.Error("Use this command in game.")
		return
	}
	index, ok := t.app.current.Load(p.UUID())
	if !ok {
		return
	}
	next := uint16(1) - index.(uint16)
	handle := tx.RemoveEntity(p)
	t.app.worlds[next].Do(func(dest *world.Tx) {
		moved := dest.AddEntity(handle).(*player.Player)
		moved.Teleport(dest.World().Spawn().Vec3Centre())
		t.app.current.Store(moved.UUID(), next)
		if !t.app.ext.WorldChanged(moved.UUID(), cinnabarext.WorldID(fmt.Sprintf("sample:world%d", next))) {
			moved.Message("World changed; the optional client part is unavailable.")
		}
	})
}

// handler removes the per-session world choice when a player leaves.
type handler struct {
	player.NopHandler
	app *application
}

// HandleQuit retires this player's application state.
func (h *handler) HandleQuit(p *player.Player) { h.app.current.Delete(p.UUID()) }
