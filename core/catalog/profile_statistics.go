package catalog

import (
	"context"
	"net/http"

	"github.com/sandertv/gophertunnel/minecraft/service/userstats"
)

// ProfileStatistics preserves unavailable values separately from a real zero.
type ProfileStatistics struct {
	MinutesPlayed     *string `json:"minutes_played,omitempty"`
	BlocksBroken      *string `json:"blocks_broken,omitempty"`
	MobsDefeated      *string `json:"mobs_defeated,omitempty"`
	DistanceTravelled *string `json:"distance_travelled,omitempty"`
}

// profileStatistics maps the typed title statistics into the existing menu contract.
func profileStatistics(ctx context.Context, client *http.Client, xuid string) (*ProfileStatistics, error) {
	stats, err := userstats.NewClient(client).Profile(ctx, xuid)
	if err != nil {
		return nil, err
	}
	return profileStatisticsView(stats), nil
}

// profileStatisticsView retains the service's precision and missing-value distinction.
func profileStatisticsView(stats *userstats.ProfileStatistics) *ProfileStatistics {
	return &ProfileStatistics{
		MinutesPlayed:     stats.MinutesPlayed,
		BlocksBroken:      stats.BlocksBroken,
		MobsDefeated:      stats.MobsDefeated,
		DistanceTravelled: stats.DistanceTravelled,
	}
}
