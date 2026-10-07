package catalog

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft/service/userstats"
)

// TestProfileStatisticsViewPreservesPrecisionAndAbsence covers the menu's typed service adapter.
func TestProfileStatisticsViewPreservesPrecisionAndAbsence(t *testing.T) {
	minutes, zero, distance := "120.5", "0", "12345.75"
	stats := profileStatisticsView(&userstats.ProfileStatistics{
		MinutesPlayed:     &minutes,
		BlocksBroken:      &zero,
		DistanceTravelled: &distance,
	})
	if stats.MinutesPlayed == nil || *stats.MinutesPlayed != minutes || stats.BlocksBroken == nil || *stats.BlocksBroken != zero || stats.MobsDefeated != nil || stats.DistanceTravelled == nil || *stats.DistanceTravelled != distance {
		t.Fatalf("statistics = %+v", stats)
	}
	raw, err := json.Marshal(stats)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(raw), `"blocks_broken":"0"`) || strings.Contains(string(raw), "mobs_defeated") {
		t.Fatalf("menu statistics = %s", raw)
	}
}
