package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"strings"
)

func chiseledBookshelfSelectors(properties []StateProperty) (books uint32, direction uint32, err error) {
	if len(properties) != 2 {
		return 0, 0, fmt.Errorf("chiseled_bookshelf requires exactly books_stored:int and direction:int")
	}
	seenBooks, seenDirection := false, false
	for _, property := range properties {
		name := property.Name
		if property.Value.Kind != ScalarInt {
			return 0, 0, fmt.Errorf("chiseled_bookshelf %s must be an int selector", name)
		}
		switch name {
		case "books_stored":
			if seenBooks || property.Value.Int < 0 || property.Value.Int > 63 {
				return 0, 0, fmt.Errorf("chiseled_bookshelf books_stored must be unique and inside 0..63")
			}
			seenBooks = true
			books = uint32(property.Value.Int)
		case "direction":
			if seenDirection || property.Value.Int < 0 || property.Value.Int > 3 {
				return 0, 0, fmt.Errorf("chiseled_bookshelf direction must be unique and inside 0..3")
			}
			seenDirection = true
			direction = uint32(property.Value.Int)
		default:
			return 0, 0, fmt.Errorf("chiseled_bookshelf has unsupported selector %q", name)
		}
	}
	if !seenBooks || !seenDirection {
		return 0, 0, fmt.Errorf("chiseled_bookshelf requires books_stored and direction selectors")
	}
	return books, direction, nil
}

func isBeeHousingName(name string) bool {
	return name == "minecraft:bee_nest" || name == "minecraft:beehive"
}

// isShelfName checks the shelf group declared in the shared compiler rules.
func isShelfName(name string) bool {
	return legacyFamilyMember(name, "family-shelf")
}

func isShelfCandidateName(name string) bool {
	return strings.HasSuffix(strings.TrimPrefix(name, "minecraft:"), "_shelf")
}

func shelfSelectors(properties []StateProperty) (direction uint32, powered uint32, shelfType uint32, err error) {
	if len(properties) != 3 {
		return 0, 0, 0, fmt.Errorf("shelf requires exactly minecraft:cardinal_direction:string, powered_bit:byte, and powered_shelf_type:int")
	}
	seenDirection, seenPowered, seenType := false, false, false
	for _, property := range properties {
		switch property.Name {
		case "minecraft:cardinal_direction":
			if seenDirection || property.Value.Kind != ScalarString {
				return 0, 0, 0, fmt.Errorf("shelf minecraft:cardinal_direction must be one unique string")
			}
			seenDirection = true
			switch property.Value.String {
			case "south":
				direction = 0
			case "west":
				direction = 1
			case "north":
				direction = 2
			case "east":
				direction = 3
			default:
				return 0, 0, 0, fmt.Errorf("shelf minecraft:cardinal_direction is outside south/west/north/east")
			}
		case "powered_bit":
			if seenPowered || property.Value.Kind != ScalarByte || property.Value.Byte > 1 {
				return 0, 0, 0, fmt.Errorf("shelf powered_bit must be one unique byte inside 0..1")
			}
			seenPowered = true
			powered = uint32(property.Value.Byte)
		case "powered_shelf_type":
			if seenType || property.Value.Kind != ScalarInt || property.Value.Int < 0 || property.Value.Int > 3 {
				return 0, 0, 0, fmt.Errorf("shelf powered_shelf_type must be one unique int inside 0..3")
			}
			seenType = true
			shelfType = uint32(property.Value.Int)
		default:
			return 0, 0, 0, fmt.Errorf("shelf has unsupported selector %q", property.Name)
		}
	}
	if !seenDirection || !seenPowered || !seenType {
		return 0, 0, 0, fmt.Errorf("shelf requires direction, powered, and type selectors")
	}
	return direction, powered, shelfType, nil
}

func beeHousingSelectors(properties []StateProperty) (direction uint32, honey uint32, err error) {
	if len(properties) != 2 {
		return 0, 0, fmt.Errorf("bee housing requires exactly direction:int and honey_level:int")
	}
	seenDirection, seenHoney := false, false
	for _, property := range properties {
		if property.Value.Kind != ScalarInt {
			return 0, 0, fmt.Errorf("bee housing %s must be an int selector", property.Name)
		}
		switch property.Name {
		case "direction":
			if seenDirection || property.Value.Int < 0 || property.Value.Int > 3 {
				return 0, 0, fmt.Errorf("bee housing direction must be unique and inside 0..3")
			}
			seenDirection = true
			direction = uint32(property.Value.Int)
		case "honey_level":
			if seenHoney || property.Value.Int < 0 || property.Value.Int > 5 {
				return 0, 0, fmt.Errorf("bee housing honey_level must be unique and inside 0..5")
			}
			seenHoney = true
			honey = uint32(property.Value.Int)
		default:
			return 0, 0, fmt.Errorf("bee housing has unsupported selector %q", property.Name)
		}
	}
	if !seenDirection || !seenHoney {
		return 0, 0, fmt.Errorf("bee housing requires direction and honey_level selectors")
	}
	return direction, honey, nil
}

func resinClumpSelector(properties []StateProperty) (uint32, error) {
	if len(properties) != 1 || properties[0].Name != "multi_face_direction_bits" {
		return 0, fmt.Errorf("resin_clump requires exactly multi_face_direction_bits:int")
	}
	property := properties[0]
	if property.Value.Kind != ScalarInt || property.Value.Int < 0 || property.Value.Int > 63 {
		return 0, fmt.Errorf("resin_clump multi_face_direction_bits must be an int inside 0..63")
	}
	return uint32(property.Value.Int), nil
}

func cactusAgeSelector(properties []StateProperty) (uint32, error) {
	if len(properties) != 1 || properties[0].Name != "age" {
		return 0, fmt.Errorf("cactus requires exactly age:int")
	}
	property := properties[0]
	if property.Value.Kind != ScalarInt || property.Value.Int < 0 || property.Value.Int > 15 {
		return 0, fmt.Errorf("cactus age must be an int inside 0..15")
	}
	return uint32(property.Value.Int), nil
}

func cakeBiteSelector(properties []StateProperty) (uint32, error) {
	if len(properties) != 1 || properties[0].Name != "bite_counter" {
		return 0, fmt.Errorf("cake requires exactly bite_counter:int")
	}
	property := properties[0]
	if property.Value.Kind != ScalarInt || property.Value.Int < 0 || property.Value.Int > 6 {
		return 0, fmt.Errorf("cake bite_counter must be an int inside 0..6")
	}
	return uint32(property.Value.Int), nil
}

func farmlandMoistureSelector(properties []StateProperty) (uint32, error) {
	if len(properties) != 1 || properties[0].Name != "moisturized_amount" {
		return 0, fmt.Errorf("farmland requires exactly moisturized_amount:int")
	}
	property := properties[0]
	if property.Value.Kind != ScalarInt || property.Value.Int < 0 || property.Value.Int > 7 {
		return 0, fmt.Errorf("farmland moisturized_amount must be an int inside 0..7")
	}
	return uint32(property.Value.Int), nil
}

func exactCanonicalInt(raw json.RawMessage, maximum int32) (uint32, bool) {
	var tagged map[string]json.RawMessage
	if err := json.Unmarshal(raw, &tagged); err != nil || len(tagged) != 2 {
		return 0, false
	}
	var kind string
	if err := json.Unmarshal(tagged["type"], &kind); err != nil || kind != "int" {
		return 0, false
	}
	var value int32
	if err := json.Unmarshal(tagged["value"], &value); err != nil || value < 0 || value > maximum {
		return 0, false
	}
	return uint32(value), true
}

func chiseledBookshelfCanonicalSelectors(stateJSON []byte) (books uint32, direction uint32, ok bool) {
	var state map[string]json.RawMessage
	if err := json.Unmarshal(stateJSON, &state); err != nil || len(state) != 2 {
		return 0, 0, false
	}
	books, booksOK := exactCanonicalInt(state["books_stored"], 63)
	direction, directionOK := exactCanonicalInt(state["direction"], 3)
	return books, direction, booksOK && directionOK
}

func beeHousingCanonicalSelectors(stateJSON []byte) (direction uint32, honey uint32, ok bool) {
	var state map[string]json.RawMessage
	if err := json.Unmarshal(stateJSON, &state); err != nil || len(state) != 2 {
		return 0, 0, false
	}
	direction, directionOK := exactCanonicalInt(state["direction"], 3)
	honey, honeyOK := exactCanonicalInt(state["honey_level"], 5)
	return direction, honey, directionOK && honeyOK
}

func shelfCanonicalSelectors(stateJSON []byte) (direction uint32, powered uint32, shelfType uint32, ok bool) {
	var state map[string]json.RawMessage
	if err := json.Unmarshal(stateJSON, &state); err != nil || len(state) != 3 {
		return 0, 0, 0, false
	}
	directionName, directionOK := exactCanonicalString(state["minecraft:cardinal_direction"])
	switch directionName {
	case "south":
		direction = 0
	case "west":
		direction = 1
	case "north":
		direction = 2
	case "east":
		direction = 3
	default:
		directionOK = false
	}
	poweredByte, poweredOK := exactCanonicalByte(state["powered_bit"])
	shelfType, typeOK := exactCanonicalInt(state["powered_shelf_type"], 3)
	return direction, uint32(poweredByte), shelfType, directionOK && poweredOK && typeOK
}

func resinClumpCanonicalSelector(stateJSON []byte) (uint32, bool) {
	var state map[string]json.RawMessage
	if err := json.Unmarshal(stateJSON, &state); err != nil || len(state) != 1 {
		return 0, false
	}
	return exactCanonicalInt(state["multi_face_direction_bits"], 63)
}

func cactusCanonicalAge(stateJSON []byte) (uint32, bool) {
	var state map[string]json.RawMessage
	if err := json.Unmarshal(stateJSON, &state); err != nil || len(state) != 1 {
		return 0, false
	}
	return exactCanonicalInt(state["age"], 15)
}

func cakeCanonicalBite(stateJSON []byte) (uint32, bool) {
	decoder := json.NewDecoder(bytes.NewReader(stateJSON))
	opening, err := decoder.Token()
	if err != nil || opening != json.Delim('{') || !decoder.More() {
		return 0, false
	}
	key, err := decoder.Token()
	if err != nil || key != "bite_counter" {
		return 0, false
	}
	bite, ok := exactCakeTaggedInt(decoder)
	if !ok || decoder.More() {
		return 0, false
	}
	closing, err := decoder.Token()
	if err != nil || closing != json.Delim('}') {
		return 0, false
	}
	var trailing any
	if err := decoder.Decode(&trailing); err != io.EOF {
		return 0, false
	}
	return bite, true
}

func farmlandCanonicalMoisture(stateJSON []byte) (uint32, bool) {
	decoder := json.NewDecoder(bytes.NewReader(stateJSON))
	opening, err := decoder.Token()
	if err != nil || opening != json.Delim('{') || !decoder.More() {
		return 0, false
	}
	key, err := decoder.Token()
	if err != nil || key != "moisturized_amount" {
		return 0, false
	}
	amount, ok := exactTaggedInt(decoder, 7)
	if !ok || decoder.More() {
		return 0, false
	}
	closing, err := decoder.Token()
	if err != nil || closing != json.Delim('}') {
		return 0, false
	}
	var trailing any
	if err := decoder.Decode(&trailing); err != io.EOF {
		return 0, false
	}
	return amount, true
}

func exactTaggedInt(decoder *json.Decoder, maximum int32) (uint32, bool) {
	opening, err := decoder.Token()
	if err != nil || opening != json.Delim('{') {
		return 0, false
	}
	var kind string
	var value int32
	seenKind, seenValue := false, false
	for decoder.More() {
		key, err := decoder.Token()
		if err != nil {
			return 0, false
		}
		switch key {
		case "type":
			if seenKind || decoder.Decode(&kind) != nil {
				return 0, false
			}
			seenKind = true
		case "value":
			if seenValue || decoder.Decode(&value) != nil {
				return 0, false
			}
			seenValue = true
		default:
			return 0, false
		}
	}
	closing, err := decoder.Token()
	if err != nil || closing != json.Delim('}') || !seenKind || !seenValue || kind != "int" || value < 0 || value > maximum {
		return 0, false
	}
	return uint32(value), true
}

func exactCakeTaggedInt(decoder *json.Decoder) (uint32, bool) {
	opening, err := decoder.Token()
	if err != nil || opening != json.Delim('{') {
		return 0, false
	}
	var kind string
	var value int32
	seenKind, seenValue := false, false
	for decoder.More() {
		key, err := decoder.Token()
		if err != nil {
			return 0, false
		}
		switch key {
		case "type":
			if seenKind || decoder.Decode(&kind) != nil {
				return 0, false
			}
			seenKind = true
		case "value":
			if seenValue || decoder.Decode(&value) != nil {
				return 0, false
			}
			seenValue = true
		default:
			return 0, false
		}
	}
	closing, err := decoder.Token()
	if err != nil || closing != json.Delim('}') || !seenKind || !seenValue || kind != "int" || value < 0 || value > 6 {
		return 0, false
	}
	return uint32(value), true
}

func signOrientation(name string, properties []StateProperty) (uint32, error) {
	values := make(map[string]uint32, 2)
	for _, property := range properties {
		propertyName := strings.TrimPrefix(property.Name, "minecraft:")
		if propertyName != "facing_direction" && propertyName != "ground_sign_direction" {
			continue
		}
		value, ok := scalarUint(property.Value)
		if !ok {
			return 0, fmt.Errorf("%s %s is not an unsigned selector", name, propertyName)
		}
		values[propertyName] = value
	}
	if strings.Contains(name, "hanging_sign") {
		facing, hasFacing := values["facing_direction"]
		rotation, hasRotation := values["ground_sign_direction"]
		if !hasFacing || facing > 5 || !hasRotation || rotation > 15 {
			return 0, fmt.Errorf("%s requires facing_direction 0..5 and ground_sign_direction 0..15", name)
		}
		// Preserve both selectors. `hanging` chooses which one controls visible
		// geometry: wall-hanging signs use the facing nibble, while ceiling
		// hanging signs use the 16-way ground rotation nibble.
		return rotation | (facing << 4), nil
	}
	if strings.HasSuffix(name, "standing_sign") || name == "standing_sign" {
		rotation, ok := values["ground_sign_direction"]
		if !ok || rotation > 15 {
			return 0, fmt.Errorf("%s requires ground_sign_direction 0..15", name)
		}
		return rotation, nil
	}
	if strings.HasSuffix(name, "wall_sign") || name == "wall_sign" {
		facing, ok := values["facing_direction"]
		if !ok || facing > 5 {
			return 0, fmt.Errorf("%s requires facing_direction 0..5", name)
		}
		return facing, nil
	}
	return 0, fmt.Errorf("unsupported sign family %s", name)
}

func scalarUint(scalar TypedScalar) (uint32, bool) {
	switch scalar.Kind {
	case ScalarByte:
		return uint32(scalar.Byte), true
	case ScalarInt:
		if scalar.Int >= 0 {
			return uint32(scalar.Int), true
		}
	case ScalarString:
		switch scalar.String {
		case "bottom", "lower", "false", "south":
			return 0, true
		case "top", "upper", "true", "west":
			return 1, true
		case "north":
			return 2, true
		case "east":
			return 3, true
		}
	}
	return 0, false
}

func orientationUint(property string, scalar TypedScalar) (uint32, bool) {
	if scalar.Kind != ScalarString {
		return scalarUint(scalar)
	}
	if property == "lever_direction" {
		values := map[string]uint32{"down_east_west": 0, "east": 1, "west": 2, "south": 3, "north": 4, "up_north_south": 5, "up_east_west": 6, "down_north_south": 7}
		value, ok := values[scalar.String]
		return value, ok
	}
	if property == "torch_facing_direction" {
		values := map[string]uint32{"unknown": 0, "west": 1, "east": 2, "north": 3, "south": 4, "top": 5}
		value, ok := values[scalar.String]
		return value, ok
	}
	if property == "pillar_axis" {
		values := map[string]uint32{"x": 0, "y": 1, "z": 2}
		value, ok := values[scalar.String]
		return value, ok
	}
	return scalarUint(scalar)
}

func selectorAliasCubeCollisionIsExact(seed CollisionSeed) bool {
	return seed.ShapeID == 1 &&
		seed.Confidence == CollisionConfidenceCollisionOnly &&
		len(seed.Boxes) == 1 &&
		seed.Boxes[0] == (CollisionBox{MaxX: 100_000_000, MaxY: 100_000_000, MaxZ: 100_000_000})
}

func exactCanonicalString(raw json.RawMessage) (string, bool) {
	var tagged map[string]json.RawMessage
	if err := json.Unmarshal(raw, &tagged); err != nil || len(tagged) != 2 {
		return "", false
	}
	var kind string
	if err := json.Unmarshal(tagged["type"], &kind); err != nil || kind != "string" {
		return "", false
	}
	var value string
	if err := json.Unmarshal(tagged["value"], &value); err != nil {
		return "", false
	}
	return value, true
}

func exactCanonicalByte(raw json.RawMessage) (byte, bool) {
	var tagged map[string]json.RawMessage
	if err := json.Unmarshal(raw, &tagged); err != nil || len(tagged) != 2 {
		return 0, false
	}
	var kind string
	if err := json.Unmarshal(tagged["type"], &kind); err != nil || kind != "byte" {
		return 0, false
	}
	var value uint8
	if err := json.Unmarshal(tagged["value"], &value); err != nil || value > 1 {
		return 0, false
	}
	return value, true
}

func selectorAliasAxisState(stateJSON []byte, hasDeprecated bool) (axisIndex, deprecated uint32, ok bool) {
	var state map[string]json.RawMessage
	wantProperties := 1
	if hasDeprecated {
		wantProperties = 2
	}
	if err := json.Unmarshal(stateJSON, &state); err != nil || len(state) != wantProperties {
		return 0, 0, false
	}
	axis, axisOK := exactCanonicalString(state["pillar_axis"])
	if !axisOK {
		return 0, 0, false
	}
	switch axis {
	case "y":
		axisIndex = 0
	case "x":
		axisIndex = 1
	case "z":
		axisIndex = 2
	default:
		return 0, 0, false
	}
	if hasDeprecated {
		var deprecatedOK bool
		deprecated, deprecatedOK = exactCanonicalInt(state["deprecated"], 3)
		if !deprecatedOK {
			return 0, 0, false
		}
	}
	return axisIndex, deprecated, true
}

// promoteReviewedSelectorAliasCubes admits only the complete, native-reviewed
// selector products below. Validation is deliberately atomic: no BREG record is
// changed until every present reviewed product has passed exact state, ID,
// projection, collision, and pre-promotion geometry checks.
