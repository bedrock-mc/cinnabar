package main

import (
	"errors"
	"flag"
	"fmt"
	"io"
	"path/filepath"
	"strings"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/world"
)

const maxPlayers = 4 // one local player plus a reconnect overlapping its predecessor

// settings are the per-world options the core passes on the command line.
type settings struct {
	dir, addr, name, gameMode, diff string
	// experiences is the directory of server Experience artifacts, empty for none; runtime is the
	// experience-runtime binary that runs them.
	experiences, runtime string
	// extensionKey, extensionAudience and extensionCXB offer client parts: the server key seed
	// file, the host:port players join by and the directory of .cxb bundles; empty for none.
	extensionKey, extensionAudience, extensionCXB string
}

func parseSettings(args []string, stderr io.Writer) (settings, error) {
	var s settings
	flags := flag.NewFlagSet("bedrock-local-server", flag.ContinueOnError)
	flags.SetOutput(stderr)
	flags.StringVar(&s.dir, "dir", "", "world data directory")
	flags.StringVar(&s.addr, "addr", "", "loopback UDP listen address")
	flags.StringVar(&s.name, "name", "World", "world display name")
	flags.StringVar(&s.gameMode, "game-mode", "survival", "survival, creative or adventure")
	flags.StringVar(&s.diff, "difficulty", "normal", "peaceful, easy, normal or hard")
	flags.StringVar(&s.experiences, "experiences", "", "directory of server Experience artifacts")
	flags.StringVar(&s.runtime, "experience-runtime", "", "experience-runtime binary; required with -experiences")
	flags.StringVar(&s.extensionKey, "extension-key", "", "server key seed file (cinnabar-cxb keygen) that signs the client part offer")
	flags.StringVar(&s.extensionAudience, "extension-audience", "", "host:port that players join by, as the client canonicalizes it")
	flags.StringVar(&s.extensionCXB, "extension-cxb", "", "directory of client part bundles (.cxb) to offer")
	if err := flags.Parse(args); err != nil {
		return settings{}, err
	}
	if s.dir == "" || s.addr == "" {
		return settings{}, errors.New("-dir and -addr are required")
	}
	if s.experiences != "" && s.runtime == "" {
		return settings{}, errors.New("-experience-runtime is required with -experiences")
	}
	if err := s.checkExtensionFlags(); err != nil {
		return settings{}, err
	}
	if _, err := s.worldGameMode(); err != nil {
		return settings{}, err
	}
	if _, err := s.worldDifficulty(); err != nil {
		return settings{}, err
	}
	return s, nil
}

// checkExtensionFlags requires the -extension flags together or not at all.
func (s settings) checkExtensionFlags() error {
	flags := []struct{ name, value string }{
		{"-extension-key", s.extensionKey},
		{"-extension-audience", s.extensionAudience},
		{"-extension-cxb", s.extensionCXB},
	}
	var set, missing []string
	for _, f := range flags {
		if f.value == "" {
			missing = append(missing, f.name)
		} else {
			set = append(set, f.name)
		}
	}
	if len(set) > 0 && len(missing) > 0 {
		return fmt.Errorf("%s also needs %s", strings.Join(set, " and "), strings.Join(missing, " and "))
	}
	return nil
}

func (s settings) worldGameMode() (world.GameMode, error) {
	switch s.gameMode {
	case "survival":
		return world.GameModeSurvival, nil
	case "creative":
		return world.GameModeCreative, nil
	case "adventure":
		return world.GameModeAdventure, nil
	}
	return nil, fmt.Errorf("unknown game mode %q", s.gameMode)
}

func (s settings) worldDifficulty() (world.Difficulty, error) {
	switch s.diff {
	case "peaceful":
		return world.DifficultyPeaceful, nil
	case "easy":
		return world.DifficultyEasy, nil
	case "normal":
		return world.DifficultyNormal, nil
	case "hard":
		return world.DifficultyHard, nil
	}
	return nil, fmt.Errorf("unknown difficulty %q", s.diff)
}

// userConfig is an offline, loopback-only server whose data lives under s.dir.
func (s settings) userConfig() server.UserConfig {
	uc := server.DefaultConfig()
	uc.Network.Address = s.addr
	uc.Network.Transport = []string{"raknet"}
	uc.Server.Name = s.name
	uc.Server.AuthEnabled = false
	uc.Server.DisableJoinQuitMessages = true
	uc.World.SaveData = true
	uc.World.Folder = filepath.Join(s.dir, "db")
	uc.Players.SaveData = true
	uc.Players.Folder = filepath.Join(s.dir, "players")
	uc.Players.MaxCount = maxPlayers
	uc.Resources.Folder = s.resourcesDir()
	return uc
}

// resourcesDir is the directory of the resource packs Dragonfly loads.
func (s settings) resourcesDir() string {
	return filepath.Join(s.dir, "resources")
}

// applyTo sets the world's gameplay defaults; they are re-applied on every start so the stored settings win over level.dat.
func (s settings) applyTo(worlds ...*world.World) {
	mode, _ := s.worldGameMode()
	difficulty, _ := s.worldDifficulty()
	for _, w := range worlds {
		w.SetDefaultGameMode(mode)
		w.SetDifficulty(difficulty)
	}
}
