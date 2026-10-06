// Command bedrock-local-server hosts one saved superflat world on dragonfly's default generators for
// the core; vanilla terrain runs on BDS instead.
// It prints "ready" once listening and reads "pause", "resume" and "stop" lines on stdin, and
// "experience reload <id>" lines when it hosts Experiences; stdin EOF and SIGINT/SIGTERM also stop
// it. docs/experience-runtime.md describes the Experiences of -experiences and the client parts of
// the -extension flags.
package main

import (
	"bufio"
	"context"
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
	"strings"
	"sync"
	"syscall"

	"github.com/df-mc/dragonfly/server/world"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/experience"
	"github.com/hashimthearab/rust-mcbe/tools/localserver/extension"
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
	cfg, err := parseSettings(args, stderr)
	if err != nil {
		return err
	}
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
	conf, err := cfg.userConfig().Config(logger)
	if err != nil {
		if exps != nil {
			err = errors.Join(err, exps.closeSupervisors())
		}
		return fmt.Errorf("configure server: %w", err)
	}
	if ext != nil {
		for i, listen := range conf.Listeners {
			conf.Listeners[i] = ext.Listener(listen)
		}
	}
	if cfg.primitiveShapes {
		for i, listen := range conf.Listeners {
			conf.Listeners[i] = primitiveListener(listen)
		}
	}
	srv := conf.New()
	worlds := []*world.World{srv.World(), srv.Nether(), srv.End()}
	cfg.applyTo(worlds...)
	cmds := commands{pause: func(paused bool) { setPaused(worlds, paused) }}
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
		running.Go(func() { host.Run(runCtx) })
		running.Go(func() { exps.store.RunFlusher(runCtx) })
		cmds.pause = func(paused bool) {
			host.Pause(paused)
			setPaused(worlds, paused)
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
		deliverClientMessages(ext, srv.Player, host, logger)
	}
	registerChatCommands()
	srv.Listen()
	accepting := make(chan struct{})
	go func() {
		defer close(accepting)
		for range srv.Accept() {
		}
	}()
	fmt.Fprintln(stdout, "ready")

	ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer stop()
	serveCommands(ctx, stdin, cmds)
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

// commands are the actions of the stdin protocol. reload is nil without Experiences, and its
// lines are then ignored like any unknown line.
type commands struct {
	pause  func(paused bool)
	reload func(id string)
}

// serveCommands runs the stdin protocol until "stop", EOF or ctx ends.
func serveCommands(ctx context.Context, stdin io.Reader, cmds commands) {
	lines := make(chan string)
	go func() {
		defer close(lines)
		scanner := bufio.NewScanner(stdin)
		for scanner.Scan() {
			lines <- strings.TrimSpace(scanner.Text())
		}
	}()
	for {
		select {
		case <-ctx.Done():
			return
		case line, ok := <-lines:
			if !ok || line == "stop" {
				return
			}
			switch line {
			case "pause":
				cmds.pause(true)
			case "resume":
				cmds.pause(false)
			default:
				if id, ok := reloadID(line); ok && cmds.reload != nil {
					cmds.reload(id)
				}
			}
		}
	}
}

// reloadID returns the id of an "experience reload <id>" line.
func reloadID(line string) (string, bool) {
	fields := strings.Fields(line)
	if len(fields) != 3 || fields[0] != "experience" || fields[1] != "reload" {
		return "", false
	}
	return fields[2], true
}

// setPaused suspends every dimension's simulation; connected players stay connected.
func setPaused(worlds []*world.World, paused bool) {
	for _, w := range worlds {
		w.SetPaused(paused)
	}
}
