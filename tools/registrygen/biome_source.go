package main

import (
	"errors"
	"fmt"

	shared "github.com/cinnabar-mc/protocolgen/generated/data"
	sharedbiome "github.com/cinnabar-mc/protocolgen/generated/data/biome"
)

// validateSharedBiomeTarget binds numeric IDs to this carrier's release and source lock.
func validateSharedBiomeTarget() error {
	if shared.MinecraftVersion != v2193GameVersion || shared.ProtocolVersion != v2193BlockProtocol {
		return fmt.Errorf("shared biome catalog target %s/%d does not match carrier %s/%d", shared.MinecraftVersion, shared.ProtocolVersion, v2193GameVersion, v2193BlockProtocol)
	}
	if shared.SourceLockSHA256 != shared.SemanticSourceLockSHA256 {
		return errors.New("shared biome catalog and release have different source locks")
	}
	if len(sharedbiome.All()) != int(shared.GeneratedCounts.Biomes) {
		return errors.New("shared biome catalog count does not match its metadata")
	}
	return nil
}
