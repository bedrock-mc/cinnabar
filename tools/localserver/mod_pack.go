package main

import (
	"archive/zip"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"os"
	"path/filepath"

	"github.com/df-mc/dragonfly/server"
	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/resource"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/experience"
)

// exportModPack validates a server artifact and exports Dragonfly's generated resource pack without listening.
func exportModPack(args []string) error {
	if len(args) != 3 {
		return errors.New("usage: export-mod-pack <artifact> <experience-runtime> <out.mcpack>")
	}
	log := slog.New(slog.DiscardHandler)
	sup, loaded, err := experience.StartSupervisor(args[1], args[0], log)
	if err != nil {
		return err
	}
	if err := sup.Close(); err != nil {
		return err
	}
	if _, err := experience.Register([]experience.Loaded{loaded}); err != nil {
		return err
	}
	return exportRegisteredPack(args[2], loaded.ID)
}

// exportRegisteredPack writes the ordinary pack of the current process's registrations without listening.
func exportRegisteredPack(path, id string) error {
	log := slog.New(slog.DiscardHandler)
	dir, err := os.MkdirTemp("", "mod-pack-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(dir)
	uc := server.DefaultConfig()
	uc.Server.AuthEnabled = false
	uc.World.SaveData, uc.Players.SaveData = false, false
	uc.Resources.Folder = filepath.Join(dir, "resources")
	conf, err := uc.Config(log)
	if err != nil {
		return err
	}
	var packErr error
	sentinel := errors.New("offline pack export")
	conf.Listeners = []func(server.Config) (server.Listener, error){func(c server.Config) (server.Listener, error) {
		if len(c.Resources) == 0 {
			// No declared art is a valid empty mod; still emit a normal empty resource pack.
			packErr = writeEmptyModPack(path, id)
		} else {
			pack := c.Resources[len(c.Resources)-1]
			file, err := os.Create(path)
			if err != nil {
				packErr = err
				return nil, sentinel
			}
			_, packErr = io.Copy(file, io.NewSectionReader(pack, 0, int64(pack.Len())))
			packErr = errors.Join(packErr, file.Close())
		}
		return nil, sentinel
	}}
	srv := conf.New()
	// No listener was created; start the lifecycle so Close can stop the temporary worlds.
	srv.Listen()
	if err := srv.Close(); err != nil {
		return fmt.Errorf("close pack builder: %w", err)
	}
	return packErr
}

// writeEmptyModPack emits a valid resource pack for a mod with no ordinary content.
func writeEmptyModPack(path, id string) error {
	version := resource.Version{1, 0, 0}
	manifest := resource.Manifest{FormatVersion: 2,
		Header:  resource.Header{Name: id, Description: "Mod resources", UUID: uuid.NewSHA1(uuid.NameSpaceURL, []byte(id+"/pack")), Version: version},
		Modules: []resource.Module{{UUID: uuid.NewSHA1(uuid.NameSpaceURL, []byte(id+"/resources")).String(), Type: "resources", Version: version}},
	}
	data, err := json.Marshal(manifest)
	if err != nil {
		return err
	}
	file, err := os.Create(path)
	if err != nil {
		return err
	}
	archive := zip.NewWriter(file)
	entry, err := archive.Create("manifest.json")
	if err == nil {
		_, err = entry.Write(data)
	}
	return errors.Join(err, archive.Close(), file.Close())
}
