package experience

import (
	"errors"
	"fmt"
	"math"
	"strings"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/block/customblock"
	"github.com/df-mc/dragonfly/server/item"
	"github.com/go-gl/mathgl/mgl64"
)

// axis is one state of a block type: its name, and its values in the order the client
// enumerates them. A bool state's values are false then true.
type axis struct {
	name    string
	values  []any
	boolean bool
}

// index is the position of v among the axis' values, if it takes v.
func (a axis) index(v StateValue) (int, bool) {
	switch {
	case a.boolean && v.Bool != nil:
		if *v.Bool {
			return 1, true
		}
		return 0, true
	case !a.boolean && v.Choice != nil:
		for i, value := range a.values {
			if value == *v.Choice {
				return i, true
			}
		}
	}
	return 0, false
}

// value is the axis' value at index i, as the protocol writes it.
func (a axis) value(i int) StateValue {
	if a.boolean {
		v := i == 1
		return StateValue{Bool: &v}
	}
	v := a.values[i].(string)
	return StateValue{Choice: &v}
}

// traitsOf returns the fork's traits for the placement states, and the states they add in the
// order the client adds them: the order of placementStates.
func traitsOf(placement []PlacementState) []customblock.Trait {
	var direction customblock.PlacementDirection
	var position customblock.PlacementPosition
	for _, p := range placement {
		switch p {
		case PlacementCardinalDirection:
			direction.CardinalDirection = true
		case PlacementFacingDirection:
			direction.FacingDirection = true
		case PlacementBlockFace:
			position.BlockFace = true
		case PlacementVerticalHalf:
			position.VerticalHalf = true
		}
	}
	var traits []customblock.Trait
	if direction.CardinalDirection || direction.FacingDirection {
		traits = append(traits, direction)
	}
	if position.BlockFace || position.VerticalHalf {
		traits = append(traits, position)
	}
	return traits
}

// stateAxes checks the states that a block of the Experience exp declares beside its placement
// traits, and returns the axes of both, the traits' first, as the runtime orders them. Their
// combinations number at most maxStateCombinations.
func stateAxes(exp string, states []StateDef, traits []customblock.Trait) ([]axis, error) {
	var axes []axis
	for _, trait := range traits {
		for _, state := range trait.States() {
			axes = append(axes, axis{name: state.Name, values: state.Values})
		}
	}
	namespace := exp + ":"
	for _, state := range states {
		name, ok := strings.CutPrefix(state.Name, namespace)
		if !ok || !isName(name) {
			return nil, fmt.Errorf("state %q is not %s<name>, the name matching ^[a-z0-9_]{1,%d}$",
				state.Name, namespace, maxNameBytes)
		}
		for _, a := range axes {
			if a.name == state.Name {
				return nil, fmt.Errorf("state %q is declared twice", state.Name)
			}
		}
		switch {
		case state.Values.Bool:
			axes = append(axes, axis{name: state.Name, values: []any{false, true}, boolean: true})
		case state.Values.Choices != nil:
			choices := *state.Values.Choices
			if len(choices) < 1 || len(choices) > maxStateValues {
				return nil, fmt.Errorf("state %q has %d values; it needs 1 to %d", state.Name,
					len(choices), maxStateValues)
			}
			values := make([]any, 0, len(choices))
			for i, choice := range choices {
				if !isName(choice) {
					return nil, fmt.Errorf("state %q value %q does not match ^[a-z0-9_]{1,%d}$",
						state.Name, choice, maxNameBytes)
				}
				for _, earlier := range choices[:i] {
					if earlier == choice {
						return nil, fmt.Errorf("state %q lists value %q twice", state.Name, choice)
					}
				}
				values = append(values, choice)
			}
			axes = append(axes, axis{name: state.Name, values: values})
		default:
			return nil, fmt.Errorf("state %q has no values", state.Name)
		}
	}
	combinations := 1
	for _, a := range axes {
		combinations *= len(a.values)
		if combinations > maxStateCombinations {
			return nil, fmt.Errorf("its states and placement traits make more than %d combinations",
				maxStateCombinations)
		}
	}
	return axes, nil
}

// isName reports whether name matches ^[a-z0-9_]{1,maxNameBytes}$.
func isName(name string) bool {
	if len(name) < 1 || len(name) > maxNameBytes {
		return false
	}
	for _, c := range []byte(name) {
		if (c < 'a' || c > 'z') && (c < '0' || c > '9') && c != '_' {
			return false
		}
	}
	return true
}

// checkCondition checks that c tests, at most maxConditionTests times, states among axes against
// values they take.
func checkCondition(c Condition, axes []axis) error {
	tests := 0
	for _, clause := range c {
		tests += len(clause)
	}
	if tests > maxConditionTests {
		return fmt.Errorf("a condition has %d tests; the limit is %d", tests, maxConditionTests)
	}
	for _, clause := range c {
		for _, test := range clause {
			i := axisIndex(axes, test.State.Name)
			if i < 0 {
				return fmt.Errorf("a condition tests state %q, which the block does not have",
					test.State.Name)
			}
			if _, ok := axes[i].index(test.State.Value); !ok {
				return fmt.Errorf("a condition tests state %q against %s, a value it does not take",
					test.State.Name, jsonText(test.State.Value))
			}
		}
	}
	return nil
}

// conditionsOverlap reports whether two validated conditions can hold in the same block state.
func conditionsOverlap(axes []axis, left, right Condition) bool {
	for _, l := range left {
		for _, r := range right {
			allowed := make(map[int]uint32)
			possible := true
			for _, clause := range [][]StateTest{l, r} {
				for _, test := range clause {
					i := axisIndex(axes, test.State.Name)
					value, _ := axes[i].index(test.State.Value)
					mask, ok := allowed[i]
					if !ok {
						mask = (1 << len(axes[i].values)) - 1
					}
					if test.Equal {
						mask &= 1 << value
					} else {
						mask &^= 1 << value
					}
					allowed[i] = mask
					if mask == 0 {
						possible = false
						break
					}
				}
				if !possible {
					break
				}
			}
			if possible {
				return true
			}
		}
	}
	return false
}

// axisIndex is the index of the axis named name, or -1.
func axisIndex(axes []axis, name string) int {
	for i, a := range axes {
		if a.name == name {
			return i
		}
	}
	return -1
}

// jsonText is v as JSON, for a message.
func jsonText(v StateValue) string {
	data, err := v.MarshalJSON()
	if err != nil {
		return "nothing"
	}
	return string(data)
}

// molang writes c as the Molang that vanilla's definitions use: q.block_state against a bool or
// a quoted string, each clause's tests joined by && and in parentheses when the clauses are
// joined by ||. An empty condition never holds and an empty clause always does.
func molang(c Condition) string {
	if len(c) == 0 {
		return "false"
	}
	clauses := make([]string, 0, len(c))
	for _, clause := range c {
		if len(clause) == 0 {
			clauses = append(clauses, "true")
			continue
		}
		tests := make([]string, 0, len(clause))
		for _, test := range clause {
			op := "!="
			if test.Equal {
				op = "=="
			}
			var literal string
			switch v := test.State.Value; {
			case v.Bool != nil:
				literal = fmt.Sprint(*v.Bool)
			case v.Choice != nil:
				literal = "'" + *v.Choice + "'"
			}
			tests = append(tests, fmt.Sprintf("q.block_state('%s') %s %s", test.State.Name, op, literal))
		}
		text := strings.Join(tests, " && ")
		if len(tests) > 1 && len(c) > 1 {
			text = "(" + text + ")"
		}
		clauses = append(clauses, text)
	}
	return strings.Join(clauses, " || ")
}

// holds reports whether c holds for the block b.
func holds(c Condition, b Block) bool {
	for _, clause := range c {
		all := true
		for _, test := range clause {
			i := axisIndex(b.t.axes, test.State.Name)
			want, _ := b.t.axes[i].index(test.State.Value)
			if (b.index(i) == want) != test.Equal {
				all = false
				break
			}
		}
		if all {
			return true
		}
	}
	return false
}

// index is the index of b's value on its type's axis i: s holds the indices in mixed radix, the
// first axis varying fastest.
func (b Block) index(i int) int {
	return int(b.s/b.t.radix[i]) % len(b.t.axes[i].values)
}

// states returns b's states, in its type's axis order.
func (b Block) states() BlockStates {
	states := make(BlockStates, len(b.t.axes))
	for i, a := range b.t.axes {
		states[i] = BlockState{Name: a.name, Value: a.value(b.index(i))}
	}
	return states
}

// errUnknownState is a state or value that a block does not take.
var errUnknownState = errors.New("unknown state")

// withStates returns b with the states given, each one of its type's, once, at a value it takes;
// the rest keep theirs.
func (b Block) withStates(states BlockStates) (Block, error) {
	seen := make(map[string]bool, len(states))
	for _, state := range states {
		i := axisIndex(b.t.axes, state.Name)
		if i < 0 {
			return b, fmt.Errorf("%w: %s has no state %q", errUnknownState, b.t.id, state.Name)
		}
		if seen[state.Name] {
			return b, fmt.Errorf("%w: state %q is set twice", errUnknownState, state.Name)
		}
		seen[state.Name] = true
		v, ok := b.t.axes[i].index(state.Value)
		if !ok {
			return b, fmt.Errorf("%w: state %q does not take %s", errUnknownState, state.Name,
				jsonText(state.Value))
		}
		b.s = b.s - uint32(b.index(i))*b.t.radix[i] + uint32(v)*b.t.radix[i]
	}
	return b, nil
}

// placer is what the vanilla placement traits read of the player who places a block: the yaw,
// the position, and the bottom and top of the bounding box.
type placer struct {
	yaw        float64
	pos        mgl64.Vec3
	minY, maxY float64
}

// placerOf reads user as a placer.
func placerOf(user item.User) placer {
	pos := user.Position()
	box := user.H().Type().BBox(user).Translate(pos)
	return placer{yaw: user.Rotation().Yaw(), pos: pos, minY: box.Min().Y(), maxY: box.Max().Y()}
}

// facingByQuarter is the facing_direction index of each yaw quarter starting south: south,
// west, north, east.
var facingByQuarter = [4]int{3, 4, 2, 5}

// value is the value that the placement trait state p takes for a block placed at pos against
// the clicked face, clicked at click, as the vanilla client's placement traits set it:
// cardinal_direction from the yaw in quarters starting south; facing_direction down when the
// block is below the placer's feet and up when it is above its head, within one block
// horizontally, else from the yaw like cardinal_direction; block_face the clicked face;
// vertical_half top for a clicked bottom face or a click above the middle of a side. The traits'
// rotation offset is 0.
func (p placer) value(state PlacementState, pos cube.Pos, face cube.Face, click mgl64.Vec3) string {
	quarter := int(math.Floor(p.yaw/90+0.5)) & 3
	faces := customblock.PlacementPosition{BlockFace: true}.States()[0].Values
	switch state {
	case PlacementCardinalDirection:
		return customblock.PlacementDirection{CardinalDirection: true}.States()[0].Values[quarter].(string)
	case PlacementFacingDirection:
		facing := facingByQuarter[quarter]
		near := func(at float64, block int) bool { return math.Abs(math.Floor(at)-float64(block)) < 2 }
		if near(p.pos.X(), pos.X()) && near(p.pos.Z(), pos.Z()) {
			switch y := float64(pos.Y()); {
			case y < p.minY:
				facing = int(cube.FaceDown)
			case y > p.maxY:
				facing = int(cube.FaceUp)
			}
		}
		return faces[facing].(string)
	case PlacementBlockFace:
		return faces[face].(string)
	case PlacementVerticalHalf:
		if face == cube.FaceDown || (face != cube.FaceUp && click.Y() > 0.5) {
			return "top"
		}
		return "bottom"
	}
	return ""
}

// placed returns b with the states of its placement traits set for its placement at pos by user
// against the clicked face, clicked at click.
func (b Block) placed(user item.User, pos cube.Pos, face cube.Face, click mgl64.Vec3) Block {
	if len(b.t.placement) == 0 {
		return b
	}
	p := placerOf(user)
	states := make(BlockStates, 0, len(b.t.placement))
	for _, state := range b.t.placement {
		name := placementStateName(state)
		value := p.value(state, pos, face, click)
		states = append(states, BlockState{Name: name, Value: StateValue{Choice: &value}})
	}
	placed, err := b.withStates(states)
	if err != nil {
		// Every value comes from the trait's own list.
		panic(err)
	}
	return placed
}

// placementStateName is the block state that the placement state adds.
func placementStateName(state PlacementState) string {
	return "minecraft:" + string(state)
}
