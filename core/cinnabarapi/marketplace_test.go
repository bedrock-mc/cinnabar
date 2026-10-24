package cinnabarapi

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
)

// modEntry supplies original synthetic selectors; no downloaded code or grants are trusted here.
func modEntry() bridgecontract.CinnabarModEntry {
	return bridgecontract.CinnabarModEntry{Slug: "fixture-mod", Version: "1.0.0", SHA256: strings.Repeat("a", 64), Bytes: 8, SentAt: "2026-10-23T00:00:00.000Z"}
}

// serverModEntry includes the upstream grant hint that native selectors deliberately omit.
type serverModEntry struct {
	bridgecontract.CinnabarModEntry
	Grants map[string]any `json:"grants"`
}

// serverMods builds schema-complete server fixtures without treating grant hints as authority.
func serverMods(entries []bridgecontract.CinnabarModEntry) []serverModEntry {
	var result []serverModEntry
	for _, entry := range entries {
		result = append(result, serverModEntry{entry, map[string]any{"commands": []string{"help"}}})
	}
	return result
}

func TestMarketplacePushIsAccountScopedAndWithdrawnOnDisconnect(t *testing.T) {
	api := newFakeAPI(t)
	api.modLibrary = []bridgecontract.CinnabarModEntry{modEntry()}
	client := newTestClient(t, api, nil)
	client.Configure(testConfig)
	session := receive(t, api.sessions)
	entry := modEntry()
	testwait.Eventually(t, testwait.DefaultTimeout, "welcome library", func() bool { return len(client.MarketplaceLibrary().Mods) == 1 })
	push := map[string]any{"type": "mod_library", "mods": serverMods([]bridgecontract.CinnabarModEntry{entry})}
	validate(t, "ServerMessage", push)
	session.send(t, push)
	testwait.Eventually(t, testwait.DefaultTimeout, "marketplace push", func() bool {
		got := client.MarketplaceLibrary()
		return got.XUID == "2535400000000001" && len(got.Mods) == 1
	})
	got := client.MarketplaceLibrary()
	got.Mods[0].Slug = "changed"
	if client.MarketplaceLibrary().Mods[0].Slug != entry.Slug {
		t.Fatal("snapshot aliases live state")
	}
	if !client.RemoveMarketplace(entry) {
		t.Fatal("current selection removal was not queued")
	}
	var removal map[string]any
	session.receive(t, &removal)
	validate(t, "ClientMessage", removal)
	if removal["type"] != "mod_library_remove" || removal["sent_at"] != entry.SentAt {
		t.Fatal(removal)
	}
	stale := entry
	stale.SentAt = "2000-01-01T00:00:00.000Z"
	if client.RemoveMarketplace(stale) {
		t.Fatal("stale selection removal was queued")
	}
	entry.Pulled = true
	push = map[string]any{"type": "mod_library", "mods": serverMods([]bridgecontract.CinnabarModEntry{entry})}
	validate(t, "ServerMessage", push)
	session.send(t, push)
	testwait.Eventually(t, testwait.DefaultTimeout, "withdrawal hint", func() bool { got := client.MarketplaceLibrary(); return len(got.Mods) == 1 && got.Mods[0].Pulled })
	client.mu.Lock()
	client.live = nil
	client.mu.Unlock()
	if got := client.MarketplaceLibrary(); got.XUID != "" || len(got.Mods) != 0 {
		t.Fatal("disconnected account retained marketplace state", got)
	}
}

func TestMarketplaceSelectorsAndPinnedCapacity(t *testing.T) {
	entry := modEntry()
	if len(admittedLibrary([]bridgecontract.CinnabarModEntry{entry})) != 1 {
		t.Fatal("valid selector refused")
	}
	if admittedLibrary([]bridgecontract.CinnabarModEntry{entry, entry}) != nil {
		t.Fatal("duplicate project admitted")
	}
	entry.Slug = "../outside"
	if admittedLibrary([]bridgecontract.CinnabarModEntry{entry}) != nil {
		t.Fatal("unsafe selector admitted")
	}
	var schema struct {
		Defs map[string]struct {
			MaxItems int `json:"maxItems"`
		} `json:"$defs"`
	}
	if err := json.Unmarshal(pinnedSchema, &schema); err != nil {
		t.Fatal(err)
	}
	if schema.Defs["ModLibraryEntries"].MaxItems != bridgecontract.CinnabarModsMaxEntries {
		t.Fatal("marketplace capacity diverged from pinned API schema")
	}
}
