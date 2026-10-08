package main

import (
	"encoding/json"
	"fmt"
)

// stateEmission preserves the state-dependent client accessors instead of a name-level default.
func stateEmission(record Record) (byte, bool, error) {
	var key string
	switch record.Name {
	case "minecraft:trial_spawner":
		key = "trial_spawner_state"
	case "minecraft:vault":
		key = "vault_state"
	case "minecraft:respawn_anchor":
		key = "respawn_anchor_charge"
	case "minecraft:sculk_sensor", "minecraft:calibrated_sculk_sensor":
		key = "sculk_sensor_phase"
	default:
		return 0, false, nil
	}
	var state map[string]canonicalScalar
	if err := json.Unmarshal(record.StateJSON, &state); err != nil {
		return 0, false, err
	}
	value, ok := state[key]
	if !ok {
		return 0, false, fmt.Errorf("%s lacks %s", record.Name, key)
	}
	if key == "vault_state" {
		// The enum-zero identity follows the pinned vault state schema.
		switch value.Value {
		case "inactive":
			return 6, true, nil
		case "active", "unlocking", "ejecting":
			return 12, true, nil
		default:
			return 0, false, fmt.Errorf("unknown vault state %v", value.Value)
		}
	}
	number, ok := value.Value.(float64)
	if !ok || number != float64(int(number)) {
		return 0, false, fmt.Errorf("invalid %s", key)
	}
	switch key {
	case "trial_spawner_state":
		// Ominous does not affect this accessor.
		switch int(number) {
		case 0, 5:
			return 0, true, nil
		case 1:
			return 4, true, nil
		case 2, 3, 4:
			return 8, true, nil
		}
	case "respawn_anchor_charge":
		// Pack metadata/mojang-blocks.json supplies the five charge values.
		const chargeStates = 5
		if number >= 0 && number < chargeStates {
			return byte(number / (chargeStates - 1) * 15), true, nil
		}
	case "sculk_sensor_phase":
		// Sculk sensors select emission from their phase state.
		if number >= 0 && number <= 2 {
			if number == 1 {
				return 1, true, nil
			}
			return 0, true, nil
		}
	}
	return 0, false, fmt.Errorf("unknown %s value %v", key, value.Value)
}
