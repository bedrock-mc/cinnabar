package experience

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math"
	"os"
	"slices"
	"strings"

	entitydata "github.com/cinnabar-mc/protocolgen/generated/data/entity"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/entity"
	"github.com/df-mc/dragonfly/server/world"
)

// These extension policy limits are checked against the runtime's generated fixture.
const (
	maxEntityTypes      = 64
	maxEntitySeats      = 16
	maxEntityProperties = 32
	maxEntities         = 256
)

// modEntityType owns immutable registration and the resource pack assembled from it.
type modEntityType struct {
	owner     string
	def       EntityDefinition
	resources world.EntityResources
	host      *Host
}

// EntityRegistry extends Dragonfly's existing registry while preserving its built-in factories.
func (r *Registry) EntityRegistry() world.EntityRegistry {
	types := entity.DefaultRegistry.Types()
	if r != nil {
		ids := make([]string, 0, len(r.entityTypes))
		for id := range r.entityTypes {
			ids = append(ids, id)
		}
		slices.Sort(ids)
		for _, id := range ids {
			types = append(types, r.entityTypes[id])
		}
	}
	return entity.DefaultRegistry.Config().New(types)
}

// newModEntityType checks all definitions and files before exposing a type to Dragonfly.
func newModEntityType(owner string, def EntityDefinition, cache *assetCache) (*modEntityType, error) {
	if !ownedWorldName(def.ID, owner) || !finitePosition(def.Collision.Min) || !finitePosition(def.Collision.Max) || def.Collision.Min.X >= def.Collision.Max.X || def.Collision.Min.Y >= def.Collision.Max.Y || def.Collision.Min.Z >= def.Collision.Max.Z {
		return nil, errors.New("invalid entity identity or collision box")
	}
	if len(def.Seats) > maxEntitySeats || len(def.Properties) > maxEntityProperties {
		return nil, errors.New("entity declaration exceeds extension policy")
	}
	steering := 0
	for _, seat := range def.Seats {
		if !finitePosition(seat.Position) || seat.RotationLimit != nil && (!finite(*seat.RotationLimit) || *seat.RotationLimit < 0) {
			return nil, errors.New("invalid entity seat")
		}
		if seat.Steers {
			steering++
		}
	}
	if steering > 1 {
		return nil, errors.New("an entity has multiple controlling seats")
	}
	seen := map[string]bool{}
	for _, p := range def.Properties {
		if !ownedWorldName(p.ID, owner) || seen[p.ID] || !finite(float64(p.Min)) || !finite(float64(p.Max)) || !finite(float64(p.Initial)) || p.Min > p.Initial || p.Initial > p.Max {
			return nil, errors.New("invalid entity property")
		}
		seen[p.ID] = true
	}
	t := &modEntityType{owner: owner, def: def}
	if err := t.buildResources(cache); err != nil {
		return nil, fmt.Errorf("entity %s: %w", def.ID, err)
	}
	return t, nil
}

// finite rejects invalid coordinates while leaving content ranges to the owning world.
func finite(v float64) bool { return !math.IsNaN(v) && !math.IsInf(v, 0) }

// finitePosition checks all coordinates before converting them to framework vectors.
func finitePosition(p Position) bool { return finite(p.X) && finite(p.Y) && finite(p.Z) }

// buildResources constructs a client identity, optional original art, and its server definition.
func (t *modEntityType) buildResources(cache *assetCache) error {
	d := t.def
	description := map[string]any{"identifier": d.ID}
	assets := map[string][]byte{}
	stem := strings.ReplaceAll(d.ID, ":", "/")
	switch d.Appearance.Type {
	case "vanilla":
		if d.Appearance.Art != nil {
			return errors.New("vanilla appearance contains custom art")
		}
		if _, ok := entitydata.Lookup(d.Appearance.ID); !ok {
			return errors.New("unknown vanilla renderer")
		}
		description["runtime_identifier"] = d.Appearance.ID
	case "custom":
		if d.Appearance.Art == nil || d.Appearance.ID != "" {
			return errors.New("invalid custom art")
		}
		art := d.Appearance.Art
		geometry, err := cache.geometry(t.owner, art.Geometry)
		if err != nil {
			return err
		}
		if _, err := cache.image(art.Texture); err != nil {
			return err
		}
		texture, err := readEntityAsset(art.Texture, maxTextureBytes)
		if err != nil {
			return err
		}
		controllers, err := readEntityAsset(art.RenderController, maxGeometryBytes)
		if err != nil {
			return err
		}
		var document struct {
			Controllers map[string]json.RawMessage `json:"render_controllers"`
		}
		if err := json.Unmarshal(controllers, &document); err != nil || len(document.Controllers) != 1 {
			return errors.New("render controller file must declare one controller")
		}
		var controller string
		for id := range document.Controllers {
			controller = id
		}
		if !strings.HasPrefix(controller, "controller.render."+t.owner+".") {
			return errors.New("render controller is outside the mod namespace")
		}
		texturePath := "textures/entity/" + stem
		assets["models/entity/"+stem+".geo.json"], assets[texturePath+".png"], assets["render_controllers/"+stem+".json"] = geometry.data, texture, controllers
		description["geometry"] = map[string]string{"default": geometry.identifier}
		description["textures"] = map[string]string{"default": texturePath}
		description["materials"] = map[string]string{"default": "entity_alphatest"}
		description["render_controllers"] = []string{controller}
	default:
		return errors.New("unknown entity appearance")
	}
	client, err := json.Marshal(map[string]any{"format_version": "1.10.0", "minecraft:client_entity": map[string]any{"description": description}})
	if err != nil {
		return err
	}
	properties := map[string]any{}
	for _, p := range d.Properties {
		properties[p.ID] = map[string]any{"type": "float", "range": []float32{p.Min, p.Max}, "default": p.Initial, "client_sync": true}
	}
	serverDescription := map[string]any{"identifier": d.ID, "is_spawnable": false, "is_summonable": false, "properties": properties}
	seats := make([]any, 0, len(d.Seats))
	control := 0
	for i, s := range d.Seats {
		seat := map[string]any{"position": []float64{s.Position.X, s.Position.Y, s.Position.Z}}
		if s.RotationLimit != nil {
			seat["lock_rider_rotation"] = *s.RotationLimit
		}
		if s.Steers {
			control = i
		}
		seats = append(seats, seat)
	}
	components := map[string]any{"minecraft:collision_box": map[string]any{"width": d.Collision.Max.X - d.Collision.Min.X, "height": d.Collision.Max.Y - d.Collision.Min.Y}, "minecraft:physics": map[string]any{"has_gravity": false}}
	if len(seats) > 0 {
		components["minecraft:rideable"] = map[string]any{"seat_count": len(seats), "controlling_seat": control, "family_types": []string{"player"}, "seats": seats}
	}
	definition, err := json.Marshal(map[string]any{"format_version": "1.20.0", "minecraft:entity": map[string]any{"description": serverDescription, "components": components}})
	if err != nil {
		return err
	}
	t.resources = world.EntityResources{ClientEntity: client, Definition: definition, Assets: assets}
	return nil
}

// readEntityAsset bounds the read before pack generation and rejects empty files.
func readEntityAsset(path string, limit int) ([]byte, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	data, err := io.ReadAll(io.LimitReader(file, int64(limit)+1))
	if err != nil {
		return nil, err
	}
	if len(data) == 0 || len(data) > limit {
		return nil, errors.New("entity asset exceeds extension policy or is empty")
	}
	return data, nil
}

// EncodeEntity returns the stable server and client identity of this type.
func (t *modEntityType) EncodeEntity() string { return t.def.ID }

// Resources returns immutable files consumed by Dragonfly before listening.
func (t *modEntityType) Resources() world.EntityResources { return t.resources }

// Transient keeps guest-owned process-local actors out of terrain storage.
func (*modEntityType) Transient() bool { return true }

// Seats adapts the registered seats without exposing mutable registration state.
func (t *modEntityType) Seats() []world.Seat {
	out := make([]world.Seat, 0, len(t.def.Seats))
	for _, s := range t.def.Seats {
		out = append(out, world.Seat{Position: s.Position.vec(), RotationLimit: s.RotationLimit, Controlling: s.Steers})
	}
	return out
}

// BBox scales the type's collision box with the instance's visual scale.
func (t *modEntityType) BBox(e world.Entity) cube.BBox {
	scale := 1.0
	if e != nil {
		scale = e.(*modEntity).Scale()
	}
	return cube.Box(t.def.Collision.Min.X*scale, t.def.Collision.Min.Y*scale, t.def.Collision.Min.Z*scale, t.def.Collision.Max.X*scale, t.def.Collision.Max.Y*scale, t.def.Collision.Max.Z*scale)
}

// DecodeNBT is unused because these entity types are transient.
func (*modEntityType) DecodeNBT(map[string]any, *world.EntityData) {}

// EncodeNBT is unused because these entity types are transient.
func (*modEntityType) EncodeNBT(*world.EntityData) map[string]any { return nil }

// Open creates a transaction-scoped adapter over the persistent framework handle.
func (t *modEntityType) Open(tx *world.Tx, h *world.EntityHandle, data *world.EntityData) world.Entity {
	return &modEntity{tx: tx, h: h, data: data, typ: t}
}

// EntityPropertyDefinitions supplies the names and ranges for indexed actor property replication.
func (t *modEntityType) EntityPropertyDefinitions() map[string]any {
	properties := make([]any, 0, len(t.def.Properties))
	for _, p := range t.def.Properties {
		properties = append(properties, map[string]any{"name": p.ID, "type": int32(1), "min": p.Min, "max": p.Max})
	}
	return map[string]any{"type": t.def.ID, "properties": properties}
}
