package xboxpresence

import (
	"encoding/json"
	"os"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/sandertv/gophertunnel/minecraft/auth"
)

// TestPresencePolicy checks the shared Rust/Go cases and the actual authentication title owner.
func TestPresencePolicy(t *testing.T) {
	if bridgecontract.PresenceTitleID != auth.AndroidConfig.TitleID {
		t.Fatal("title presence policy differs from Android authentication")
	}
	data, err := os.ReadFile("../../crates/bridge/data/presence-activities.json")
	if err != nil {
		t.Fatal(err)
	}
	var cases []struct {
		State
		Activity string `json:"activity"`
	}
	if err := json.Unmarshal(data, &cases); err != nil {
		t.Fatal(err)
	}
	if len(cases) != 40 {
		t.Fatal("incomplete activity policy cases")
	}
	for _, c := range cases {
		if got := c.ID(); got != c.Activity {
			t.Fatalf("activity = %s, want %s", got, c.Activity)
		}
	}
}
