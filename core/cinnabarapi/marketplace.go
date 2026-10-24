package cinnabarapi

import (
	"encoding/json"
	"regexp"
	"slices"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

var modSlug = regexp.MustCompile(`^[a-z0-9][a-z0-9-]{1,38}[a-z0-9]$`)
var modDigest = regexp.MustCompile(`^[0-9a-f]{64}$`)

// admittedLibrary bounds transport hints without treating them as signed installation authority.
func admittedLibrary(mods []bridgecontract.CinnabarModEntry) []bridgecontract.CinnabarModEntry {
	if len(mods) > bridgecontract.CinnabarModsMaxEntries {
		return nil
	}
	seen := make(map[string]bool)
	for _, mod := range mods {
		if !modSlug.MatchString(mod.Slug) || !modDigest.MatchString(mod.SHA256) || len(mod.Version) > 32 || len(mod.SentAt) > 40 || seen[mod.Slug] {
			return nil
		}
		seen[mod.Slug] = true
	}
	return slices.Clone(mods)
}

// RemoveMarketplace queues an exact conditional removal on the already authenticated connection.
func (c *Client) RemoveMarketplace(entry bridgecontract.CinnabarModEntry) bool {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.live == nil {
		return false
	}
	for _, current := range c.live.modLibrary {
		if current.Slug != entry.Slug || current.SHA256 != entry.SHA256 || current.SentAt != entry.SentAt {
			continue
		}
		body, _ := json.Marshal(struct {
			Type   string `json:"type"`
			Slug   string `json:"slug"`
			SHA256 string `json:"sha256"`
			SentAt string `json:"sent_at"`
		}{"mod_library_remove", entry.Slug, entry.SHA256, entry.SentAt})
		select {
		case c.live.out <- body:
			return true
		default:
			return false
		}
	}
	return false
}

// MarketplaceLibrary returns a detached snapshot belonging only to the live authenticated account.
func (c *Client) MarketplaceLibrary() bridgecontract.CinnabarModLibrary {
	c.mu.Lock()
	defer c.mu.Unlock()
	result := bridgecontract.CinnabarModLibrary{Mods: []bridgecontract.CinnabarModEntry{}}
	if c.live != nil && ValidXUID(c.live.ownershipXUID) {
		result.XUID = c.live.ownershipXUID
		result.Mods = append(result.Mods, c.live.modLibrary...)
	}
	return result
}
