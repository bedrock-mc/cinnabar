package experience

import (
	"encoding/json"
	"errors"
	"fmt"
	"image"
	"io"
	"math"
	"os"
	"path/filepath"
	"slices"
	"strings"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/block/customblock"
)

// blockPixels is a block's edge in pixels, the largest coordinate of a box.
const blockPixels = 16

// maxQuarterTurns is the most quarter turns about one axis.
const maxQuarterTurns = 3

// assetCache holds the files that registration has read: decoded textures by path, and
// geometries by path and by identifier, which must be one file.
type assetCache struct {
	images     map[string]image.Image
	geometries map[string]*geometryFile
	files      map[string]string
}

func newAssetCache() *assetCache {
	return &assetCache{
		images:     make(map[string]image.Image),
		geometries: make(map[string]*geometryFile),
		files:      make(map[string]string),
	}
}

// image decodes the texture at path, or returns the one already decoded.
func (c *assetCache) image(path string) (image.Image, error) {
	if img, ok := c.images[path]; ok {
		return img, nil
	}
	img, err := loadTexture(path)
	if err != nil {
		return nil, fmt.Errorf("texture %s: %w", filepath.Base(path), err)
	}
	c.images[path] = img
	return img, nil
}

// geometryFile is a block's .geo.json: what the pack carries and the names it defines.
type geometryFile struct {
	// identifier is geometry.<experience id>.<name>.
	identifier, name string
	data             []byte
	bones            map[string]bool
	// instances are the material instances its cube faces draw with: a face's
	// material_instance, else the face's own name.
	instances []string
}

// geometry reads the geometry file at path for the Experience exp: one geometry, in its
// namespace, whose identifier no other file of the Experience has.
func (c *assetCache) geometry(exp, path string) (*geometryFile, error) {
	g, ok := c.geometries[path]
	if !ok {
		var err error
		if g, err = readGeometry(exp, path); err != nil {
			return nil, fmt.Errorf("geometry %s: %w", filepath.Base(path), err)
		}
		c.geometries[path] = g
	}
	if file, ok := c.files[g.identifier]; ok && file != path {
		return nil, fmt.Errorf("geometry %q is in two files", g.identifier)
	}
	c.files[g.identifier] = path
	return g, nil
}

// readGeometry reads and parses the geometry file at path, as the runtime's load/geometry.rs
// does.
func readGeometry(exp, path string) (*geometryFile, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	data, err := io.ReadAll(io.LimitReader(f, maxGeometryBytes+1))
	if err != nil {
		return nil, err
	}
	if len(data) > maxGeometryBytes {
		return nil, fmt.Errorf("it exceeds %d bytes", maxGeometryBytes)
	}
	var file struct {
		Geometries []struct {
			Description struct {
				Identifier string `json:"identifier"`
			} `json:"description"`
			Bones []struct {
				Name  *string `json:"name"`
				Cubes []struct {
					UV json.RawMessage `json:"uv"`
				} `json:"cubes"`
			} `json:"bones"`
		} `json:"minecraft:geometry"`
	}
	if err := json.Unmarshal(data, &file); err != nil {
		return nil, fmt.Errorf("parsing it: %w", err)
	}
	if len(file.Geometries) != 1 {
		return nil, fmt.Errorf("it holds %d geometries; it must hold one", len(file.Geometries))
	}
	geometry := file.Geometries[0]
	identifier := geometry.Description.Identifier
	namespace := "geometry." + exp + "."
	name, ok := strings.CutPrefix(identifier, namespace)
	if !ok || !isName(name) {
		return nil, fmt.Errorf("geometry %q is outside namespace %q: it must be %s<name>, the name "+
			"matching ^[a-z0-9_]{1,%d}$", identifier, namespace, namespace, maxNameBytes)
	}
	g := &geometryFile{identifier: identifier, name: name, data: data, bones: make(map[string]bool)}
	instances := make(map[string]bool)
	for _, bone := range geometry.Bones {
		if bone.Name == nil {
			return nil, fmt.Errorf("geometry %q has a bone without a name", identifier)
		}
		g.bones[*bone.Name] = true
		for _, c := range bone.Cubes {
			var perFace map[string]struct {
				MaterialInstance *string `json:"material_instance"`
			}
			if json.Unmarshal(c.UV, &perFace) != nil || perFace == nil {
				// Box UV maps every face.
				for _, face := range faceNames {
					instances[face] = true
				}
				continue
			}
			for _, face := range faceNames {
				if uv, ok := perFace[face]; ok {
					instance := face
					if uv.MaterialInstance != nil {
						instance = *uv.MaterialInstance
					}
					instances[instance] = true
				}
			}
		}
	}
	for instance := range instances {
		g.instances = append(g.instances, instance)
	}
	slices.Sort(g.instances)
	return g, nil
}

// faceNames are the face material slots and instances, in the runtime's order.
var faceNames = []string{"up", "down", "north", "south", "east", "west"}

// visualType is the look of a block with a visual: its base properties, its permutations and
// what the resource pack carries for them.
type visualType struct {
	base         customblock.Properties
	permutations []permutationType
	// geometry is the base geometry file, nil for the full cube, and further those of the
	// permutations, by name.
	geometry []byte
	further  map[string][]byte
	// flipbooks animates textures by texture key.
	flipbooks map[string]customblock.Flipbook
	// icon is the texture key that the block's item shows.
	icon string
}

// permutationType is one permutation: its condition and the properties it sets.
type permutationType struct {
	when       Condition
	properties customblock.Properties
}

// visualBuilder checks the visual of one block and builds its visualType, giving each distinct
// texture and flipbook of the block its own texture key.
type visualBuilder struct {
	exp    string
	t      *blockType
	cache  *assetCache
	v      *visualType
	keys   map[string]string // by path and flipbook
	shapes map[*geometryFile]bool
}

// newVisual checks def's visual and permutations, as the runtime's load/block_type.rs does, and
// builds them into t.
func newVisual(exp string, def BlockDef, t *blockType, cache *assetCache) error {
	b := &visualBuilder{
		exp: exp, t: t, cache: cache, keys: make(map[string]string),
		v: &visualType{flipbooks: make(map[string]customblock.Flipbook)},
	}
	visual := def.Visual
	var shape *geometryFile
	if visual.Geometry != nil {
		var err error
		if shape, err = cache.geometry(exp, *visual.Geometry); err != nil {
			return err
		}
		b.v.geometry = shape.data
		b.v.base.Geometry = shape.identifier
	} else {
		b.v.base.Cube = true
	}
	materials, err := b.materials(visual.Materials, shape)
	if err != nil {
		return err
	}
	b.v.base.Textures = materials
	b.v.icon = iconKey(visual.Materials, materials)
	if b.v.base.BoneVisibility, err = b.bones(visual.Bones, shape); err != nil {
		return err
	}
	b.v.base.CollisionBox, b.v.base.SelectionBox = fullCube, fullCube
	if visual.Collision != nil {
		if b.v.base.CollisionBox, err = pixelBox("collision", *visual.Collision); err != nil {
			return err
		}
	}
	if visual.Selection != nil {
		if b.v.base.SelectionBox, err = pixelBox("selection", *visual.Selection); err != nil {
			return err
		}
	}
	if visual.Rotation != nil {
		if b.v.base.Rotation, err = quarterTurns(*visual.Rotation); err != nil {
			return err
		}
	}
	if len(def.Permutations) > maxPermutations {
		return fmt.Errorf("it declares %d permutations; the limit is %d", len(def.Permutations),
			maxPermutations)
	}
	for i, p := range def.Permutations {
		permutation, err := b.permutation(p, shape, visual)
		if err != nil {
			return fmt.Errorf("permutation %d: %w", i, err)
		}
		if p.Rotation != nil && *p.Rotation == (QuarterTurns{}) {
			for _, earlier := range b.v.permutations {
				if earlier.properties.Rotation != (cube.Pos{}) && conditionsOverlap(t.axes, earlier.when, permutation.when) {
					return fmt.Errorf("permutation %d: its zero rotation cannot replace an earlier permutation", i)
				}
			}
		}
		b.v.permutations = append(b.v.permutations, permutation)
	}
	t.visual = b.v
	return nil
}

// permutation checks one permutation against the block's base shape and visual.
func (b *visualBuilder) permutation(p Permutation, base *geometryFile, visual *Visual) (permutationType, error) {
	if len(p.When) == 0 {
		return permutationType{}, errors.New("its condition never holds")
	}
	if p.Geometry == nil && p.Materials == nil && p.Bones == nil && p.Collision == nil &&
		p.Selection == nil && p.Rotation == nil {
		return permutationType{}, errors.New("it sets nothing")
	}
	if err := checkCondition(p.When, b.t.axes); err != nil {
		return permutationType{}, err
	}
	out := permutationType{when: p.When}
	shape := base
	if p.Geometry != nil {
		g, err := b.cache.geometry(b.exp, *p.Geometry)
		if err != nil {
			return permutationType{}, err
		}
		shape = g
		out.properties.Geometry = g.identifier
		if base == nil || g.identifier != base.identifier {
			if b.v.further == nil {
				b.v.further = make(map[string][]byte)
			}
			b.v.further[g.name] = g.data
		}
	}
	if p.Materials != nil {
		materials, err := b.materials(*p.Materials, shape)
		if err != nil {
			return permutationType{}, err
		}
		out.properties.Textures = materials
	} else if err := covered(visual.Materials, shape); err != nil {
		// The block's materials must still cover a geometry of the permutation's own.
		return permutationType{}, err
	}
	if p.Bones != nil {
		bones, err := b.bones(*p.Bones, shape)
		if err != nil {
			return permutationType{}, err
		}
		out.properties.BoneVisibility = bones
		if shape != nil {
			// Bone overrides preserve the geometry they apply to.
			out.properties.Geometry = shape.identifier
		}
	}
	var err error
	if p.Collision != nil {
		if out.properties.CollisionBox, err = pixelBox("collision", *p.Collision); err != nil {
			return permutationType{}, err
		}
	}
	if p.Selection != nil {
		if out.properties.SelectionBox, err = pixelBox("selection", *p.Selection); err != nil {
			return permutationType{}, err
		}
	}
	if p.Rotation != nil {
		if out.properties.Rotation, err = quarterTurns(*p.Rotation); err != nil {
			return permutationType{}, err
		}
		// A zero rotation reads as none, so it cannot undo the block's own.
		if out.properties.Rotation == (cube.Pos{}) && visual.Rotation != nil &&
			*visual.Rotation != (QuarterTurns{}) {
			return permutationType{}, errors.New("its zero rotation cannot replace the visual's rotation")
		}
	}
	return out, nil
}

// materials checks materials for shape, the full cube when nil: each instance "*", a face, or
// one the geometry draws with, once, and together covering every instance it draws with. It
// returns them as the fork's materials by instance.
func (b *visualBuilder) materials(materials []Material, shape *geometryFile) (map[string]customblock.Material, error) {
	if len(materials) < 1 || len(materials) > maxMaterials {
		return nil, fmt.Errorf("it lists %d materials; it needs 1 to %d", len(materials), maxMaterials)
	}
	out := make(map[string]customblock.Material, len(materials))
	for _, m := range materials {
		if m.Instance != allFaces && !slices.Contains(faceNames, m.Instance) &&
			(shape == nil || !slices.Contains(shape.instances, m.Instance)) {
			return nil, fmt.Errorf("material instance %q is neither %q, a face, nor one its geometry draws with",
				m.Instance, allFaces)
		}
		if _, ok := out[m.Instance]; ok {
			return nil, fmt.Errorf("material instance %q is listed twice", m.Instance)
		}
		key, err := b.texture(m)
		if err != nil {
			return nil, fmt.Errorf("material instance %q: %w", m.Instance, err)
		}
		out[m.Instance] = customblock.NewMaterial(key, renderMethod(m.RenderMethod))
	}
	if err := covered(materials, shape); err != nil {
		return nil, err
	}
	return out, nil
}

// texture decodes m's texture, checks its flipbook, and returns its texture key: one per
// distinct texture and flipbook of the block.
func (b *visualBuilder) texture(m Material) (string, error) {
	img, err := b.cache.image(m.Path)
	if err != nil {
		return "", err
	}
	var flipbook *customblock.Flipbook
	if m.Flipbook != nil {
		f, err := checkFlipbook(*m.Flipbook, img)
		if err != nil {
			return "", err
		}
		flipbook = &f
	}
	identity := m.Path + "\x00" + jsonOfFlipbook(m.Flipbook)
	if key, ok := b.keys[identity]; ok {
		return key, nil
	}
	ns, name, _ := strings.Cut(b.t.id, ":")
	key := fmt.Sprintf("%s.%s.%d", ns, name, len(b.keys))
	b.keys[identity] = key
	b.t.textures[key] = img
	if flipbook != nil {
		b.v.flipbooks[key] = *flipbook
	}
	return key, nil
}

// jsonOfFlipbook writes f, or nothing, as a key.
func jsonOfFlipbook(f *Flipbook) string {
	if f == nil {
		return ""
	}
	data, _ := json.Marshal(f)
	return string(data)
}

// checkFlipbook checks a flipbook of the strip img: a whole number of square frames, of which it
// shows only ones there are, each for at least a tick.
func checkFlipbook(f Flipbook, img image.Image) (customblock.Flipbook, error) {
	if f.TicksPerFrame < 1 {
		return customblock.Flipbook{}, errors.New("its flipbook shows a frame for 0 ticks")
	}
	if len(f.Frames) > maxFlipbookFrames {
		return customblock.Flipbook{}, fmt.Errorf("its flipbook lists %d frames; the limit is %d",
			len(f.Frames), maxFlipbookFrames)
	}
	w, h := img.Bounds().Dx(), img.Bounds().Dy()
	if w <= 0 || h%w != 0 {
		return customblock.Flipbook{}, fmt.Errorf("its flipbook strip is %d×%d pixels, not whole square frames", w, h)
	}
	count := uint32(h / w)
	frames := make([]int, len(f.Frames))
	for i, frame := range f.Frames {
		if frame >= count {
			return customblock.Flipbook{}, fmt.Errorf("its flipbook shows frame %d of a strip of %d", frame, count)
		}
		frames[i] = int(frame)
	}
	return customblock.Flipbook{TicksPerFrame: int(f.TicksPerFrame), Frames: frames, NoBlend: !f.BlendFrames}, nil
}

// covered checks that materials give every instance that shape, or the full cube, draws with a
// texture: by name, or through "*".
func covered(materials []Material, shape *geometryFile) error {
	listed := func(instance string) bool {
		return slices.ContainsFunc(materials, func(m Material) bool { return m.Instance == instance })
	}
	if listed(allFaces) {
		return nil
	}
	drawn := faceNames
	if shape != nil {
		drawn = shape.instances
	}
	for _, instance := range drawn {
		if !listed(instance) {
			return fmt.Errorf("material instance %q is drawn but has no material, and there is no %q",
				instance, allFaces)
		}
	}
	return nil
}

// bones checks bone visibilities, at most maxBones, each of a bone of shape, once, and returns
// them as Molang by bone.
func (b *visualBuilder) bones(bones []BoneVisibility, shape *geometryFile) (map[string]string, error) {
	if len(bones) > maxBones {
		return nil, fmt.Errorf("it sets the visibility of %d bones; the limit is %d", len(bones), maxBones)
	}
	if len(bones) == 0 {
		return nil, nil
	}
	out := make(map[string]string, len(bones))
	for _, bone := range bones {
		if shape == nil {
			return nil, fmt.Errorf("bone %q has a visibility, but the full cube has no bones", bone.Bone)
		}
		if !shape.bones[bone.Bone] {
			return nil, fmt.Errorf("geometry %q has no bone %q", shape.identifier, bone.Bone)
		}
		if _, ok := out[bone.Bone]; ok {
			return nil, fmt.Errorf("bone %q has two visibilities", bone.Bone)
		}
		if err := checkCondition(bone.Visible, b.t.axes); err != nil {
			return nil, fmt.Errorf("bone %q: %w", bone.Bone, err)
		}
		out[bone.Bone] = molang(bone.Visible)
	}
	return out, nil
}

// iconKey is the texture key of the material that the block's item shows: "*", else the first.
func iconKey(materials []Material, built map[string]customblock.Material) string {
	instance := materials[0].Instance
	if _, ok := built[allFaces]; ok {
		instance = allFaces
	}
	return built[instance].Encode()["texture"].(string)
}

// renderMethod is the fork's render method of m.
func renderMethod(m RenderMethod) customblock.Method {
	switch m {
	case RenderAlphaTest:
		return customblock.AlphaTestRenderMethod()
	case RenderBlend:
		return customblock.BlendRenderMethod()
	case RenderDoubleSided:
		return customblock.DoubleSidedRenderMethod()
	}
	return customblock.OpaqueRenderMethod()
}

// pixelBox is a box within the block in blocks: finite, at least 0 and at most 16 pixels, and
// not empty on any axis.
func pixelBox(what string, b PixelBox) (cube.BBox, error) {
	for _, a := range []struct {
		axis   string
		lo, hi float32
	}{{"x", b.Min.X, b.Max.X}, {"y", b.Min.Y, b.Max.Y}, {"z", b.Min.Z, b.Max.Z}} {
		finite := !math.IsNaN(float64(a.lo)) && !math.IsInf(float64(a.lo), 0) &&
			!math.IsNaN(float64(a.hi)) && !math.IsInf(float64(a.hi), 0)
		if !finite || a.lo < 0 || a.lo >= a.hi || a.hi > blockPixels {
			return cube.BBox{}, fmt.Errorf("its %s box spans %v to %v on %s; it needs 0 ≤ min < max ≤ %d",
				what, a.lo, a.hi, a.axis, blockPixels)
		}
	}
	pixel := func(v float32) float64 { return float64(v) / blockPixels }
	return cube.Box(pixel(b.Min.X), pixel(b.Min.Y), pixel(b.Min.Z), pixel(b.Max.X), pixel(b.Max.Y), pixel(b.Max.Z)), nil
}

// quarterTurns is a rotation of 0 to 3 quarter turns about each axis.
func quarterTurns(q QuarterTurns) (cube.Pos, error) {
	if q.X > maxQuarterTurns || q.Y > maxQuarterTurns || q.Z > maxQuarterTurns {
		return cube.Pos{}, fmt.Errorf("its rotation turns (%d, %d, %d); each axis takes 0 to %d",
			q.X, q.Y, q.Z, maxQuarterTurns)
	}
	return cube.Pos{int(q.X), int(q.Y), int(q.Z)}, nil
}
