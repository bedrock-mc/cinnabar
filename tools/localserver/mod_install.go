package main

import (
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
)

// discoverMods finds and validates the installed artifacts shared by the server and dev client.
func (s *settings) discoverMods() error {
	entries, err := os.ReadDir(filepath.Join(s.dir, "mods"))
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	for _, entry := range entries {
		if !entry.IsDir() || strings.HasPrefix(entry.Name(), ".") {
			continue
		}
		root := filepath.Join(s.dir, "mods", entry.Name())
		artifact := filepath.Join(root, "server")
		bundle := filepath.Join(root, "client", entry.Name()+".cxb")
		for _, path := range []string{filepath.Join(artifact, manifestFile), bundle} {
			if info, err := os.Stat(path); err != nil || !info.Mode().IsRegular() {
				return fmt.Errorf("installed mod %s: missing regular artifact %s", entry.Name(), path)
			}
		}
		s.modArtifacts = append(s.modArtifacts, artifact)
		s.modClients = append(s.modClients, bundle)
	}
	return nil
}

// listModClients writes the installed client bundle paths without starting or configuring a server.
func listModClients(args []string, stdout io.Writer) error {
	if len(args) != 1 {
		return fmt.Errorf("usage: list-mod-clients <world>")
	}
	s := settings{dir: args[0], modClients: []string{}}
	if err := s.discoverMods(); err != nil {
		return err
	}
	return json.NewEncoder(stdout).Encode(s.modClients)
}

// loadMods adds installed package artifacts to the existing Experience and client-part hosts.
func (s *settings) loadMods() error {
	if err := s.discoverMods(); err != nil {
		return err
	}
	if len(s.modArtifacts) == 0 {
		return nil
	}
	if s.runtime == "" {
		self, err := os.Executable()
		if err != nil {
			return err
		}
		s.runtime = filepath.Join(filepath.Dir(self), "experience-runtime"+filepath.Ext(self))
	}
	if s.extensionAudience == "" {
		s.extensionAudience = s.addr
	}
	if s.extensionKey == "" {
		s.extensionKey = filepath.Join(s.dir, "mod-server.seed")
		if _, err := os.Stat(s.extensionKey); errors.Is(err, os.ErrNotExist) {
			seed := make([]byte, 32)
			if _, err := rand.Read(seed); err != nil {
				return err
			}
			file, err := os.OpenFile(s.extensionKey, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o600)
			if err != nil {
				return err
			}
			_, writeErr := file.WriteString(hex.EncodeToString(seed))
			if err := errors.Join(writeErr, file.Close()); err != nil {
				return err
			}
		} else if err != nil {
			return err
		}
	}
	return nil
}
