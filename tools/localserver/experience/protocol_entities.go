package experience

import (
	"encoding/json"
	"fmt"
)

// EntityHandle identifies one process-local actor in a world opening.
type EntityHandle struct {
	ID         uint64 `json:"id"`
	Generation uint64 `json:"generation"`
}

// EntityRotation stores yaw and pitch in degrees.
type EntityRotation struct {
	Yaw   float64 `json:"yaw"`
	Pitch float64 `json:"pitch"`
}

// EntityBox is the unscaled collision box relative to the actor base.
type EntityBox struct {
	Min Position `json:"min"`
	Max Position `json:"max"`
}

// EntitySeat defines an attachment offset and optional steering authority.
type EntitySeat struct {
	Position      Position `json:"position"`
	RotationLimit *float64 `json:"rotation_limit"`
	Steers        bool     `json:"steers"`
}

// EntityArt names validated absolute resource files.
type EntityArt struct {
	Geometry         string `json:"geometry"`
	Texture          string `json:"texture"`
	RenderController string `json:"render_controller"`
}

// EntityAppearance selects a built-in renderer or owned resource files.
type EntityAppearance struct {
	Type string     `json:"type"`
	ID   string     `json:"id,omitempty"`
	Art  *EntityArt `json:"art,omitempty"`
}

// EntityPropertyDef declares one synchronized floating-point property.
type EntityPropertyDef struct {
	ID      string  `json:"id"`
	Min     float32 `json:"min"`
	Max     float32 `json:"max"`
	Initial float32 `json:"initial"`
}

// EntityProperty carries one value in declaration order.
type EntityProperty struct {
	ID    string  `json:"id"`
	Value float32 `json:"value"`
}

// EntityDefinition is a validated registered actor type.
type EntityDefinition struct {
	ID         string              `json:"id"`
	Collision  EntityBox           `json:"collision"`
	Appearance EntityAppearance    `json:"appearance"`
	Seats      []EntitySeat        `json:"seats"`
	Properties []EntityPropertyDef `json:"properties"`
}

// EntityState is the guest-visible mutable state of an owned actor.
type EntityState struct {
	Handle     EntityHandle     `json:"handle"`
	Kind       string           `json:"kind"`
	Position   Position         `json:"position"`
	Rotation   EntityRotation   `json:"rotation"`
	Velocity   Position         `json:"velocity"`
	NameTag    string           `json:"name_tag"`
	Scale      float32          `json:"scale"`
	Variant    int32            `json:"variant"`
	Properties []EntityProperty `json:"properties"`
}

// EntityRider identifies the player occupying one seat.
type EntityRider struct {
	Player string `json:"player"`
	Seat   uint32 `json:"seat"`
}

// EntitySnapshot retains the revision and occupancy needed to revalidate a commit.
type EntitySnapshot struct {
	State    EntityState   `json:"state"`
	Revision uint64        `json:"revision"`
	Riders   []EntityRider `json:"riders"`
}

// EntityInput is an observed steering intent, never a client position.
type EntityInput struct {
	Rider    EntityRider `json:"rider"`
	Sideways float32     `json:"sideways"`
	Forward  float32     `json:"forward"`
	Yaw      float64     `json:"yaw"`
	Pitch    float64     `json:"pitch"`
	Jump     bool        `json:"jump"`
	Sneak    bool        `json:"sneak"`
}

// EntityInteraction retains the hand and item at the moment of interaction.
type EntityInteraction struct {
	Player string     `json:"player"`
	Hand   string     `json:"hand"`
	Item   *ItemStack `json:"item"`
}

// EntityEvent is one ordered observation from an actor transaction.
type EntityEvent struct {
	Type   string       `json:"type"`
	Player string       `json:"player,omitempty"`
	Hand   string       `json:"hand,omitempty"`
	Item   *ItemStack   `json:"item,omitempty"`
	Rider  *EntityRider `json:"rider,omitempty"`
	Input  *EntityInput `json:"input,omitempty"`
}

// MarshalJSON retains the nullable item field for interaction variants.
func (e EntityEvent) MarshalJSON() ([]byte, error) {
	if e.Type == "interact" || e.Type == "attacked" {
		return json.Marshal(struct {
			variantTag
			EntityInteraction
		}{variantTag{e.Type}, EntityInteraction{e.Player, e.Hand, e.Item}})
	}
	type plain EntityEvent
	return json.Marshal(plain(e))
}

// EntityCall carries an observation of an owned entity.
type EntityCall struct {
	Handle EntityHandle `json:"handle"`
	Event  EntityEvent  `json:"event"`
}

// EntityOp carries one staged spawn, update, despawn, mount or dismount.
type EntityOp struct {
	Type     string
	State    *EntityState
	Handle   EntityHandle
	Seat     uint32
	Teleport bool
}

// MarshalJSON encodes only the fields belonging to the selected command.
func (e EntityOp) MarshalJSON() ([]byte, error) {
	switch e.Type {
	case "spawn":
		return json.Marshal(struct {
			variantTag
			State *EntityState `json:"state"`
		}{variantTag{e.Type}, e.State})
	case "update":
		return json.Marshal(struct {
			variantTag
			State    *EntityState `json:"state"`
			Teleport bool         `json:"teleport"`
		}{variantTag{e.Type}, e.State, e.Teleport})
	case "despawn":
		return json.Marshal(struct {
			variantTag
			Handle EntityHandle `json:"handle"`
		}{variantTag{e.Type}, e.Handle})
	case "mount":
		return json.Marshal(struct {
			variantTag
			Handle EntityHandle `json:"handle"`
			Seat   uint32       `json:"seat"`
		}{variantTag{e.Type}, e.Handle, e.Seat})
	case "dismount":
		return json.Marshal(variantTag{e.Type})
	}
	return nil, fmt.Errorf("unknown entity command %q", e.Type)
}

// UnmarshalJSON checks the selected command's shape before commit validation.
func (e *EntityOp) UnmarshalJSON(data []byte) error {
	tag, err := unionTag(data)
	if err != nil {
		return err
	}
	*e = EntityOp{Type: tag}
	switch tag {
	case "spawn":
		var w struct {
			variantTag
			State *EntityState `json:"state"`
		}
		err = decodeStrict(data, &w)
		e.State = w.State
	case "update":
		var w struct {
			variantTag
			State    *EntityState `json:"state"`
			Teleport bool         `json:"teleport"`
		}
		err = decodeStrict(data, &w)
		e.State, e.Teleport = w.State, w.Teleport
	case "despawn":
		var w struct {
			variantTag
			Handle EntityHandle `json:"handle"`
		}
		err = decodeStrict(data, &w)
		e.Handle = w.Handle
	case "mount":
		var w struct {
			variantTag
			Handle EntityHandle `json:"handle"`
			Seat   uint32       `json:"seat"`
		}
		err = decodeStrict(data, &w)
		e.Handle, e.Seat = w.Handle, w.Seat
	case "dismount":
		var w variantTag
		err = decodeStrict(data, &w)
	default:
		return fmt.Errorf("unknown entity command %q", tag)
	}
	return err
}

// EntityCommand wraps the entity command in the adapter's operation union.
type EntityCommand struct {
	Action EntityOp `json:"action"`
}
type entityCallWire struct {
	variantTag
	*EntityCall
}
type entityCommandWire struct {
	variantTag
	*EntityCommand
}
