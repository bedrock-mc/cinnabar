package main

import (
	"fmt"

	shared "github.com/cinnabar-mc/protocolgen/generated/data"
	sharedblock "github.com/cinnabar-mc/protocolgen/generated/data/block"
	"github.com/cinnabar-mc/protocolgen/generated/data/registry"
)

// sharedBlockProperties joins by state hash and checks the identifier as well.
func sharedBlockProperties(record Record) (sharedblock.Properties, error) {
	state, ok := sharedblock.StateByHash(record.NetworkHash)
	if !ok {
		return sharedblock.Properties{}, fmt.Errorf("shared catalog has no block state %#x (%s)", record.NetworkHash, record.Name)
	}
	block, ok := sharedblock.BlockAt(state.Block)
	if !ok || block.Name != record.Name {
		return sharedblock.Properties{}, fmt.Errorf("shared catalog identifier does not match %s at %#x", record.Name, record.NetworkHash)
	}
	properties, ok := sharedblock.PropertiesAt(state.Properties)
	if !ok {
		return sharedblock.Properties{}, fmt.Errorf("shared catalog has no properties for %s", record.Name)
	}
	return properties, nil
}

// validateSharedTarget prevents a catalog update from silently changing this carrier's target.
func validateSharedTarget() error {
	if registry.SourceLockSHA256 != shared.SourceLockSHA256 {
		return fmt.Errorf("shared registry and property catalog have different source locks")
	}
	if shared.MinecraftVersion != v2193GameVersion || shared.ProtocolVersion != v2193BlockProtocol || sharedblock.StateCount() != v2193BlockStateCount {
		return fmt.Errorf("shared catalog target %s/%d with %d states does not match carrier %s/%d with %d states", shared.MinecraftVersion, shared.ProtocolVersion, sharedblock.StateCount(), v2193GameVersion, v2193BlockProtocol, v2193BlockStateCount)
	}
	return nil
}
