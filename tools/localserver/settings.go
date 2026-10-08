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

	"github.com/hashimthearab/rust-mcbe/tools/localserver/extension"
)

const defaultChunkWorkers = 4

const maxPlayers = 4 // one local player plus a reconnect overlapping its predecessor

// settings are the per-world options the core passes on the command line.
type settings struct {
	primitiveShapes, cameraTest, actorBurst bool
	terrainFixture, terrainFixtureGenerate  bool
	actorBurstArtwork                       int
	terrainFixtureRadius                    int
	opaqueOverdraw                          bool
	dir, addr, name, gameMode, diff         string
	generator                               string
	seed                                    int64
	pregenRadius, chunkWorkers              int
	generationStats                         bool
	// experiences is the directory of server Experience artifacts, empty for none; runtime is the
	// experience-runtime binary that runs them.
	experiences, runtime string
	// extensionKey, extensionAudience and extensionCXB offer client parts: the server key seed
	// file, the host:port players join by and the directory of .cxb bundles; empty for none.
	extensionKey, extensionAudience, extensionCXB string
	// extensionMedia is a directory served over loopback HTTPS at extensionMediaAddr for client
	// part media; empty for none.
	extensionMedia, extensionMediaAddr string
}

func parseSettings(args []string, stderr io.Writer) (settings, error) {
	var s settings
	flags := flag.NewFlagSet("bedrock-local-server", flag.ContinueOnError)
	flags.SetOutput(stderr)
	flags.BoolVar(&s.terrainFixture, "terrain-fixture", false, "serve deterministic synthetic hills, caves, trees and water")
	flags.BoolVar(&s.terrainFixtureGenerate, "terrain-fixture-generate", false, "generate a new synthetic terrain database and exit")
	flags.IntVar(&s.terrainFixtureRadius, "terrain-fixture-radius", terrainDefaultRadius, "synthetic pregeneration radius in chunks")
	flags.BoolVar(&s.primitiveShapes, "primitive-shapes", false, "emit a debug-shape gallery; /shapes, /shapes update, /shapes clear")
	flags.StringVar(&s.dir, "dir", "", "world data directory")
	flags.StringVar(&s.addr, "addr", "", "loopback UDP listen address")
	flags.StringVar(&s.name, "name", "World", "world display name")
	flags.StringVar(&s.gameMode, "game-mode", "survival", "survival, creative or adventure")
	flags.StringVar(&s.diff, "difficulty", "normal", "peaceful, easy, normal or hard")
	flags.StringVar(&s.generator, "generator", "flat", "normal or flat terrain")
	flags.Int64Var(&s.seed, "seed", 0, "world seed")
	flags.IntVar(&s.pregenRadius, "pregen-radius", 0, "generate and save normal overworld chunks around spawn before listening (0 disables)")
	flags.IntVar(&s.chunkWorkers, "chunk-workers", defaultChunkWorkers, "background chunk generation workers per dimension (1..16)")
	flags.BoolVar(&s.generationStats, "generation-stats", false, "report normal generation counts and CPU-path elapsed time at shutdown")
	flags.BoolVar(&s.cameraTest, "camera-test", false, "enable /cameratest spline, inline, aim and clear fixtures")
	flags.IntVar(&s.actorBurstArtwork, "actor-burst-artwork", 0, "offer an original resource pack with N textures; /actorburst N artwork")
	flags.BoolVar(&s.actorBurst, "actor-burst", false, "enable /actorburst N shared|skins|geometry and /actorburst clear")
	flags.BoolVar(&s.opaqueOverdraw, "opaque-overdraw", false, "generate a fixed foliage, forest canopy and cave rendering fixture")
	flags.StringVar(&s.experiences, "experiences", "", "directory of server Experience artifacts")
	flags.StringVar(&s.runtime, "experience-runtime", "", "experience-runtime binary; required with -experiences")
	flags.StringVar(&s.extensionKey, "extension-key", "", "server key seed file (cinnabar-cxb keygen) that signs the client part offer")
	flags.StringVar(&s.extensionAudience, "extension-audience", "", "host:port that players join by, as the client canonicalizes it")
	flags.StringVar(&s.extensionCXB, "extension-cxb", "", "directory of client part bundles (.cxb) to offer")
	flags.StringVar(&s.extensionMedia, "extension-media", "", "directory of client part media served over loopback HTTPS; needs the -extension flags")
	flags.StringVar(&s.extensionMediaAddr, "extension-media-addr", extension.DefaultMediaAddr, "IPv4 loopback ip:port of the -extension-media server")
	if err := flags.Parse(args); err != nil {
		return settings{}, err
	}
	if s.actorBurstArtwork < 0 || s.actorBurstArtwork > actorBurstLimit {
		return settings{}, fmt.Errorf("actor burst artwork must be 0..%d", actorBurstLimit)
	}
	if s.pregenRadius < 0 || s.pregenRadius > 512 {
		return settings{}, errors.New("pregen radius must be 0..512 chunks")
	}
	if s.chunkWorkers < 1 || s.chunkWorkers > 16 {
		return settings{}, errors.New("chunk workers must be 1..16")
	}
	if s.pregenRadius > 0 && (s.generator != "normal" || s.terrainFixture || s.opaqueOverdraw || s.terrainFixtureGenerate) {
		return settings{}, errors.New("-pregen-radius requires -generator normal without a synthetic fixture")
	}
	if s.generator != "normal" && s.generator != "flat" {
		return settings{}, fmt.Errorf("unknown generator %q", s.generator)
	}
	if s.dir == "" || (s.addr == "" && !s.terrainFixtureGenerate) {
		return settings{}, errors.New("-dir and -addr are required; generation needs only -dir")
	}
	if s.terrainFixtureRadius < terrainMinRadius || s.terrainFixtureRadius > terrainMaxRadius {
		return settings{}, fmt.Errorf("terrain fixture radius must be %d..%d", terrainMinRadius, terrainMaxRadius)
	}
	if s.opaqueOverdraw && (s.terrainFixture || s.terrainFixtureGenerate) {
		return settings{}, errors.New("-opaque-overdraw and -terrain-fixture select different generators")
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

// checkExtensionFlags requires the -extension flags together or not at all, and -extension-media
// only with them.
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
	if s.extensionMedia != "" {
		if len(set) == 0 {
			return errors.New("-extension-media also needs -extension-key, -extension-audience and -extension-cxb")
		}
		if _, err := extension.MediaOrigin(s.extensionMediaAddr); err != nil {
			return err
		}
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
	if s.opaqueOverdraw {
		uc.World.SaveData = false
		uc.Players.SaveData = false
	}
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
		if s.terrainFixture {
			freezeTerrainFixture(w)
		}
		if s.opaqueOverdraw {
			freezeOpaqueOverdrawWorld(w)
		}
	}
}
