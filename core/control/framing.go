package control

import "encoding/json"

// frameBudget leaves headroom under MaxFrameLen for the response envelope.
const frameBudget = MaxFrameLen - 2048

// encodedLen measures JSON bytes, treating an unencodable value as over budget.
func encodedLen(value any) int {
	payload, err := json.Marshal(value)
	if err != nil {
		return frameBudget + 1
	}
	return len(payload)
}
