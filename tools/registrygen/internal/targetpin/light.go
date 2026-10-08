// Package targetpin reads carrier identities from the repository target manifest.
package targetpin

import (
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

// LightHash finds the target manifest above the working directory and reads its light pin.
func LightHash() (string, error) {
	return Hash("light_registry")
}

// BlockHash reads the block-registry pin from the repository target manifest.
func BlockHash() (string, error) {
	return Hash("block_registry")
}

// Hash finds the target manifest above the working directory and validates one carrier pin.
func Hash(carrier string) (string, error) {
	dir, err := os.Getwd()
	if err != nil {
		return "", err
	}
	for {
		data, err := os.ReadFile(filepath.Join(dir, "assets", "bedrock-target.json"))
		if err == nil {
			var target struct {
				Hashes map[string]string `json:"hashes"`
			}
			if err := json.Unmarshal(data, &target); err != nil {
				return "", err
			}
			hash := target.Hashes[carrier]
			decoded, err := hex.DecodeString(hash)
			if err != nil || len(decoded) != 32 || hash != strings.ToLower(hash) {
				return "", fmt.Errorf("invalid %s pin", carrier)
			}
			return hash, nil
		}
		if !os.IsNotExist(err) {
			return "", err
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			return "", fmt.Errorf("bedrock target manifest not found")
		}
		dir = parent
	}
}
