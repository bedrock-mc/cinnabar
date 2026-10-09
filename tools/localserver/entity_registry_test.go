package main

import (
	"io"
	"log/slog"
	"testing"
)

func TestConfiguredServerUsesSupportedEntityRegistry(t *testing.T) {
	for _, generator := range []string{"normal", "flat"} {
		t.Run(generator, func(t *testing.T) {
			s := settings{dir: t.TempDir(), addr: "127.0.0.1:0", generator: generator, seed: 42}
			conf, err := s.userConfig().Config(slog.New(slog.NewTextHandler(io.Discard, nil)))
			if err != nil {
				t.Fatal(err)
			}
			srv := conf.New()
			srv.Listen()
			defer srv.Close()
			registry := srv.World().EntityRegistry()
			for _, name := range []string{"minecraft:cow", "minecraft:pig", "minecraft:sheep", "minecraft:chicken"} {
				if _, ok := registry.Lookup(name); ok {
					t.Errorf("configured %s server registers experimental animal %s", generator, name)
				}
			}
			for _, name := range []string{"minecraft:item", "minecraft:tnt", "minecraft:arrow"} {
				if _, ok := registry.Lookup(name); !ok {
					t.Errorf("configured %s server cannot create supported entity %s", generator, name)
				}
			}
		})
	}
}
