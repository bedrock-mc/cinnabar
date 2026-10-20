// Command bedrock-local-server hosts saved Dragonfly worlds with flat or natural terrain,
// or opt-in synthetic terrain and opaque-overdraw fixtures.
// It prints "ready" once listening and reads "pause", "resume" and "stop" lines on stdin, and
// "experience reload <id>" lines when it hosts Experiences; stdin EOF and SIGINT/SIGTERM also stop
// it. Started with only -standby, it warms world generation until a "start <JSON args>" line gives
// it a world without -experiences. docs/experience-runtime.md describes the Experiences of -experiences and the client parts of
// the -extension flags.
package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"log/slog"
	"maps"
	"os"
	"os/signal"
	"path/filepath"
	"slices"
	"sync"
	"syscall"

	_ "github.com/bedrock-mc/vanilla-gen/block"
	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/google/uuid"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/experience"
	"github.com/hashimthearab/rust-mcbe/tools/localserver/extension"
	"github.com/hashimthearab/rust-mcbe/tools/localserver/hostbridge"
	"github.com/hashimthearab/rust-mcbe/tools/localserver/internal/hostcontract"
)

// experienceDataDir is the directory under -dir that holds the Experiences' private data.
const experienceDataDir = "experience-data"

// manifestFile marks an immediate subdirectory of -experiences as an artifact.
const manifestFile = "experience.toml"

func main() {
	if err := run(os.Args[1:], os.Stdin, os.Stdout, os.Stderr); err != nil {
		fmt.Fprintln(os.Stderr, "bedrock-local-server:", err)
		os.Exit(1)
	}
}

func run(args []string, stdin io.Reader, stdout, stderr io.Writer) error {
	standby := len(args) == 1 && args[0] == standbyFlag
	if standby {
		go warmGeneration()
		started, rest, ok, err := awaitStandbyStart(stdin)
		if err != nil || !ok {
			return err
		}
		args, stdin = started, rest
	}
	cfg, err := parseSettings(args, stderr)
	if err != nil {
		return err
	}
	if cfg.terrainFixtureGenerate {
		return generateTerrainFixture(cfg, stdout)
	}
	if standby && cfg.experiences != "" {
		// Its warm-up has finalised the block registry that Experience blocks register in.
		return errors.New("a standby server cannot host -experiences")
	}
	pending := cfg.startGenerators()
	defer pending.discard()
	logger := slog.New(slog.NewTextHandler(stderr, nil))
	// The offer's marker pack must be written before the resource packs load.
	var ext *extension.Server
	if cfg.extensionKey != "" {
		if ext, err = startClientParts(cfg, logger); err != nil {
			return err
		}
		if cfg.extensionMedia != "" {
			media, err := startMedia(cfg, logger)
			if err != nil {
				return err
			}
			defer media.Close()
		}
	} else if err := extension.RemoveMarkerPack(cfg.resourcesDir()); err != nil {
		return err
	}
	var exps *experiences
	if cfg.experiences != "" {
		if exps, err = startExperiences(cfg, logger); err != nil {
			return err
		}
	} else if err := experience.CheckDisabled(filepath.Join(cfg.dir, experienceDataDir)); err != nil {
		return err
	}
	pending.begin()
	conf, err := cfg.userConfig().Config(logger)
	if err != nil {
		if exps != nil {
			err = errors.Join(err, exps.closeSupervisors())
		}
		return fmt.Errorf("configure server: %w", err)
	}
	if cfg.actorBurstArtwork > 0 {
		pack, err := actorBurstArtworkPack(cfg.actorBurstArtwork)
		if err != nil {
			if exps != nil {
				err = errors.Join(err, exps.closeSupervisors())
			}
			return fmt.Errorf("prepare actor artwork fixture: %w", err)
		}
		conf.Resources = append(conf.Resources, pack)
	}
	if cfg.totemTexture {
		pack, err := totemTexturePack()
		if err != nil {
			if exps != nil {
				err = errors.Join(err, exps.closeSupervisors())
			}
			return fmt.Errorf("prepare totem texture fixture: %w", err)
		}
		conf.Resources = append(conf.Resources, pack)
	}
	generators, err := pending.configure(&conf)
	if err != nil {
		if exps != nil {
			err = errors.Join(err, exps.closeSupervisors())
		}
		return err
	}
	defer generators.close()
	if cfg.generationStats && len(generators) > 0 {
		defer meterGeneration(&conf, stdout)()
	}
	for dim, generator := range generators {
		logger.Info("world generation acceleration", "dimension", dim, "status", generator.AccelerationStatus())
	}
	_, spawnErr := os.Stat(filepath.Join(cfg.dir, "db", "level.dat"))
	firstWorld := errors.Is(spawnErr, fs.ErrNotExist)
	generator, normal := generators[world.Overworld]
	if firstWorld && normal {
		// Saved before the worlds open, so the first tick finds a spawn instead of generating a
		// column to search one.
		settings := conf.WorldProvider.Settings()
		settings.Spawn = generator.DefaultSpawn(world.Overworld)
		conf.WorldProvider.SaveSettings(settings)
	}
	nativeWorld, err := hostbridge.New(conf.MaxPlayers)
	if err != nil {
		return err
	}
	for i, listen := range conf.Listeners {
		conf.Listeners[i] = nativeWorld.WrapLocal(listen)
	}
	conf.Listeners = append(conf.Listeners, nativeWorld.Factory)
	if ext != nil {
		for i, listen := range conf.Listeners {
			conf.Listeners[i] = ext.Listener(listen)
		}
	}
	if cfg.cameraTest {
		for i, listen := range conf.Listeners {
			conf.Listeners[i] = cameraTestListener(listen)
		}
	}
	if cfg.totemTest || cfg.totemTexture {
		for i, listen := range conf.Listeners {
			conf.Listeners[i] = totemTestListener(listen)
		}
	}
	if cfg.primitiveShapes {
		for i, listen := range conf.Listeners {
			conf.Listeners[i] = primitiveListener(listen)
		}
	}
	if cfg.actorBurst || cfg.actorBurstArtwork > 0 {
		for i, listen := range conf.Listeners {
			conf.Listeners[i] = actorBurstListener(listen, cfg.actorBurstArtwork)
		}
	}
	cfg.configureTerrainFixture(&conf)
	cfg.configureOpaqueOverdraw(&conf)
	conf.ChunkLoadWorkers = cfg.chunkWorkers
	ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer stop()
	lines, stopped := readCommands(ctx, stdin)
	if cfg.pregenRadius > 0 {
		startup, cancelStartup := startupContext(ctx, stopped)
		defer cancelStartup()
		if err := preparePregeneration(startup, conf.WorldProvider, conf.Generator(world.Overworld), firstWorld, cfg.pregenRadius, cfg.chunkWorkers, stdout); err != nil {
			if exps != nil {
				err = errors.Join(err, exps.closeSupervisors())
			}
			return errors.Join(err, conf.WorldProvider.Close(), conf.PlayerProvider.Close())
		}
	}
	cfg.configureLocalPlayers(&conf, nativeWorld.RemotePlayer)
	var srv *server.Server
	for i, listen := range conf.Listeners {
		conf.Listeners[i] = flySpeedListener(listen, func(id uuid.UUID) (*world.EntityHandle, bool) {
			return srv.Player(id)
		})
	}
	srv = conf.New()
	worlds := []*world.World{srv.World(), srv.Nether(), srv.End()}
	cfg.applyTo(worlds...)
	pauses := &pauseState{worlds: worlds}
	warmup := func() {}
	if firstWorld && normal {
		warmup = warmSpawn(srv.World(), pauses)
	}
	cmds := commands{pause: pauses.request}
	cmds.nativeBridge = func(body string) {
		var bootstrap hostcontract.Bootstrap
		// Optional remote extension/Experience policy remains unqualified; do not inherit local-only behavior.
		if ext != nil || exps != nil || len(body) > hostcontract.MAX_CONTROL_BYTES || json.Unmarshal([]byte(body), &bootstrap) != nil ||
			nativeWorld.Start(ctx, bootstrap) != nil {
			logger.Warn("native world bridge bootstrap rejected")
		}
	}
	var host *experience.Host
	var running sync.WaitGroup
	runCtx, stopRunning := context.WithCancel(context.Background())
	defer stopRunning()
	if exps != nil {
		// Without client parts every staged client message is dropped and counted.
		var channels experience.ClientChannels
		if ext != nil {
			channels = ext
		}
		host = experience.NewHost(exps.reg, exps.store, exps.sups, filepath.Base(cfg.dir), channels, logger)
		// Building the server registered its crafting recipes, after the helpers first loaded
		// without them: each loads again with them.
		for id := range exps.sups {
			if err := host.Reload(id); err != nil {
				logger.Error("experience reload with the server's recipes failed", "experience", id, "err", err)
			}
		}
		running.Go(func() { host.Run(runCtx) })
		running.Go(func() { exps.store.RunFlusher(runCtx) })
		cmds.pause = func(paused bool) {
			host.Pause(paused)
			pauses.request(paused)
		}
		cmds.reload = func(id string) {
			if err := host.Reload(id); err != nil {
				logger.Error("experience reload failed", "experience", id, "err", err)
				return
			}
			logger.Info("experience reloaded", "experience", id)
		}
	}
	if ext != nil {
		deliverClientPartEvents(ext, srv.Player, host, logger)
	}
	registerChatCommands()
	srv.Listen()
	accepting := make(chan struct{})
	go func() {
		defer close(accepting)
		for p := range srv.Accept() {
			warmup()
			if hostbridge.IsRemote(p.Addr()) {
				p.Handle(&visitorHandler{host: host})
			} else {
				p.Handle(&localPlayerHandler{host: host})
			}
		}
	}()
	fmt.Fprintln(stdout, "ready")

	serveCommandLines(ctx, lines, cmds)
	closeErr := srv.Close()
	<-accepting
	if host != nil {
		// The worlds have stopped, so no hook changes the store once the Host flushes it.
		if err := host.Close(); err != nil {
			closeErr = errors.Join(closeErr, fmt.Errorf("close experiences: %w", err))
		}
		stopRunning()
		running.Wait()
	}
	return closeErr
}

// experiences are the started Experiences of -experiences: their supervisors by Experience id,
// their registered blocks, and the store of their private data.
type experiences struct {
	reg   *experience.Registry
	store *experience.Store
	sups  map[string]*experience.Supervisor
}

// startExperiences discovers the artifacts of cfg.experiences, opens the store under cfg.dir and
// starts one helper per artifact. It checks that every installed Experience and block is still
// provided, then registers the blocks before server.Config.New and records their IDs together.
// On error no helper is left running.
func startExperiences(cfg settings, log *slog.Logger) (_ *experiences, err error) {
	dirs, err := discoverArtifacts(cfg.experiences)
	if err != nil {
		return nil, err
	}
	store, err := experience.OpenStore(filepath.Join(cfg.dir, experienceDataDir))
	if err != nil {
		return nil, err
	}
	e := &experiences{store: store, sups: make(map[string]*experience.Supervisor, len(dirs))}
	defer func() {
		if err != nil {
			err = errors.Join(err, e.closeSupervisors())
		}
	}()
	loaded := make([]experience.Loaded, 0, len(dirs))
	dirOf := make(map[string]string, len(dirs))
	for _, dir := range dirs {
		sup, l, err := experience.StartSupervisor(cfg.runtime, dir, log)
		if err != nil {
			return nil, err
		}
		if first, ok := dirOf[l.ID]; ok {
			return nil, errors.Join(fmt.Errorf("experience %q is in both %s and %s", l.ID, first, dir), sup.Close())
		}
		dirOf[l.ID] = dir
		e.sups[l.ID] = sup
		loaded = append(loaded, l)
	}
	if err := store.ValidateInstalled(loaded); err != nil {
		return nil, err
	}
	if e.reg, err = experience.Register(loaded); err != nil {
		return nil, err
	}
	if err := store.SetInstalled(loaded); err != nil {
		return nil, err
	}
	return e, nil
}

// closeSupervisors stops every helper. It is for startup failures; once a Host owns the
// supervisors, closing the Host closes them.
func (e *experiences) closeSupervisors() error {
	var errs []error
	for _, id := range slices.Sorted(maps.Keys(e.sups)) {
		errs = append(errs, e.sups[id].Close())
	}
	return errors.Join(errs...)
}

// discoverArtifacts returns the immediate subdirectories of root that hold an experience.toml, as
// absolute paths in byte order of their names.
func discoverArtifacts(root string) ([]string, error) {
	root, err := filepath.Abs(root)
	if err != nil {
		return nil, fmt.Errorf("resolve -experiences: %w", err)
	}
	entries, err := os.ReadDir(root) // sorted by name
	if err != nil {
		return nil, fmt.Errorf("read -experiences: %w", err)
	}
	var dirs []string
	for _, entry := range entries {
		if !entry.IsDir() {
			continue
		}
		dir := filepath.Join(root, entry.Name())
		// Any manifest counts, a symlink too, so the runtime reports what is wrong with it.
		if _, err := os.Lstat(filepath.Join(dir, manifestFile)); errors.Is(err, fs.ErrNotExist) {
			continue
		} else if err != nil {
			return nil, err
		}
		dirs = append(dirs, dir)
	}
	return dirs, nil
}
