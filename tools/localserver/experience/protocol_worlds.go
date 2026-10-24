package experience

import (
	"encoding/json"
	"fmt"
	"github.com/cinnabar-mc/cinnabar/tools/localserver/cinnabarext"
)

// WorldDefinitionID names a definition registered by a mod at load time.
type WorldDefinitionID string

// DimensionType is an environment profile; protocol dimension numbers remain in Dragonfly.
type DimensionType string

// Generator selects a host-owned terrain generator.
type Generator string

const (
	DimensionOverworld DimensionType = "overworld"
	DimensionNether    DimensionType = "nether"
	DimensionEnd       DimensionType = "end"
	GeneratorEmpty     Generator     = "empty"
	GeneratorFlat      Generator     = "flat"
)

// WorldSettings describes the generator, environment and save policy of an instance.
type WorldSettings struct {
	Generator  Generator     `json:"generator"`
	Dimension  DimensionType `json:"dimension"`
	Persistent bool          `json:"persistent"`
}

// WorldDefinition is immutable after registration.
type WorldDefinition struct {
	ID       WorldDefinitionID `json:"id"`
	Settings WorldSettings     `json:"settings"`
}

// WorldInstance requests an opening of a stable instance using a registered definition.
type WorldInstance struct {
	ID         cinnabarext.WorldID `json:"id"`
	Definition WorldDefinitionID   `json:"definition"`
}

// WorldRegistration declares definitions and startup instances before player admission.
type WorldRegistration struct {
	Definitions []WorldDefinition `json:"definitions"`
	Startup     []WorldInstance   `json:"startup"`
}

// Position is a finite world-space position in blocks.
type Position struct {
	X float64 `json:"x"`
	Y float64 `json:"y"`
	Z float64 `json:"z"`
}

// PlayerLocation identifies one admitted location, including its departure fence.
type PlayerLocation struct {
	World    WorldHandle `json:"world"`
	Epoch    uint64      `json:"epoch"`
	Position Position    `json:"position"`
}

// PlayerEvent is an ordered observation. Type selects Location or Before and After.
type PlayerEvent struct {
	Type     string          `json:"type"`
	Location *PlayerLocation `json:"location,omitempty"`
	Before   *PlayerLocation `json:"before,omitempty"`
	After    *PlayerLocation `json:"after,omitempty"`
}

// PlayerCall delivers an immutable observation even if the player has moved again.
type PlayerCall struct {
	Player string      `json:"player"`
	Event  PlayerEvent `json:"event"`
}

// StoreScope selects the callback's player, world instance, or installing mod.
type StoreScope string

const (
	StorePlayer   StoreScope = "player"
	StoreInstance StoreScope = "instance"
	StoreMod      StoreScope = "mod"
)

// StoreKey selects a versioned record in one of the three authorized scopes.
type StoreKey struct {
	Scope  StoreScope `json:"scope"`
	Schema uint32     `json:"schema"`
}

// StateRecord is a snapshotted record and revision. Data is lowercase hex, absent when nil.
type StateRecord struct {
	Key      StoreKey `json:"key"`
	Revision uint64   `json:"revision"`
	Data     *string  `json:"data"`
}

// WorldContext contains only the installing mod's instances and authorized private records.
type WorldContext struct {
	Current   WorldHandle     `json:"current"`
	Player    *PlayerLocation `json:"player"`
	Instances []WorldHandle   `json:"instances"`
	Stores    []StateRecord   `json:"stores"`
}

// WorldOp is a lifecycle-only result. Type selects Instance or the Transfer fields.
type WorldOp struct {
	Type     string         `json:"type"`
	Instance WorldHandle    `json:"-"`
	Create   *WorldInstance `json:"-"`
	Player   string         `json:"-"`
	Position Position       `json:"-"`
}

// WorldActionOp contains one coordinator command.
type WorldActionOp struct {
	Action WorldOp `json:"action"`
}

// StoreWriteOp replaces one scoped record; nil deletes it.
type StoreWriteOp struct {
	Key  StoreKey `json:"key"`
	Data *string  `json:"data"`
}

// Concrete wire forms retain strict decoding of every union member.
type playerCallWire struct {
	variantTag
	*PlayerCall
}
type worldActionWire struct {
	variantTag
	*WorldActionOp
}
type storeWriteWire struct {
	variantTag
	*StoreWriteOp
}

// MarshalJSON writes the selected lifecycle action without unused fields.
func (o WorldOp) MarshalJSON() ([]byte, error) {
	switch o.Type {
	case "create":
		return json.Marshal(struct {
			variantTag
			Instance *WorldInstance `json:"instance"`
		}{variantTag{o.Type}, o.Create})
	case "unload":
		return json.Marshal(struct {
			variantTag
			Instance WorldHandle `json:"instance"`
		}{variantTag{o.Type}, o.Instance})
	case "transfer":
		return json.Marshal(struct {
			variantTag
			Player   string      `json:"player"`
			Instance WorldHandle `json:"instance"`
			Position Position    `json:"position"`
		}{variantTag{o.Type}, o.Player, o.Instance, o.Position})
	}
	return nil, fmt.Errorf("unknown world operation %q", o.Type)
}

// UnmarshalJSON rejects unknown actions and fields before coordinator validation.
func (o *WorldOp) UnmarshalJSON(data []byte) error {
	tag, err := unionTag(data)
	if err != nil {
		return err
	}
	*o = WorldOp{Type: tag}
	switch tag {
	case "create":
		var wire struct {
			variantTag
			Instance WorldInstance `json:"instance"`
		}
		if err := decodeStrict(data, &wire); err != nil {
			return err
		}
		o.Create = &wire.Instance
	case "unload":
		var wire struct {
			variantTag
			Instance WorldHandle `json:"instance"`
		}
		if err := decodeStrict(data, &wire); err != nil {
			return err
		}
		o.Instance = wire.Instance
	case "transfer":
		var wire struct {
			variantTag
			Player   string      `json:"player"`
			Instance WorldHandle `json:"instance"`
			Position Position    `json:"position"`
		}
		if err := decodeStrict(data, &wire); err != nil {
			return err
		}
		o.Player, o.Instance, o.Position = wire.Player, wire.Instance, wire.Position
	default:
		return fmt.Errorf("unknown world operation %q", tag)
	}
	return nil
}

// Shared enum lists are checked against the exhaustive Rust fixtures.
var dimensions = []DimensionType{DimensionOverworld, DimensionNether, DimensionEnd}
var generators = []Generator{GeneratorEmpty, GeneratorFlat}
var storeScopes = []StoreScope{StorePlayer, StoreInstance, StoreMod}

// MarshalJSON rejects dimension types outside the shared contract.
func (d DimensionType) MarshalJSON() ([]byte, error) { return marshalEnum(d, dimensions) }

// UnmarshalJSON accepts only declared dimension profiles.
func (d *DimensionType) UnmarshalJSON(data []byte) error { return unmarshalEnum(data, d, dimensions) }

// MarshalJSON rejects generators outside the shared contract.
func (g Generator) MarshalJSON() ([]byte, error) { return marshalEnum(g, generators) }

// UnmarshalJSON accepts only declared generators.
func (g *Generator) UnmarshalJSON(data []byte) error { return unmarshalEnum(data, g, generators) }

// MarshalJSON rejects store scopes outside the shared contract.
func (s StoreScope) MarshalJSON() ([]byte, error) { return marshalEnum(s, storeScopes) }

// UnmarshalJSON accepts only declared store scopes.
func (s *StoreScope) UnmarshalJSON(data []byte) error { return unmarshalEnum(data, s, storeScopes) }
