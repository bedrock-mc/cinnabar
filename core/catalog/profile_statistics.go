package catalog

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"math"
	"net/http"
	"strconv"
	"strings"

	"github.com/sandertv/gophertunnel/minecraft/auth"
)

// The names and order match vanilla profile statistics. The Xbox SDK REST contract is
// https://github.com/microsoft/xbox-live-api/blob/main/Source/Services/Stats/user_statistics_service.cpp.
var profileStatisticNames = [...]string{"MinutesPlayed", "BlockBrokenTotal", "MobKilled.IsMonster.1", "DistanceTravelled"}

const profileStatisticsEndpoint = "https://userstats.xboxlive.com"

// ProfileStatistics preserves unavailable values separately from a real zero.
// Values are numeric strings because vanilla exposes double statistics to OreUI.
type ProfileStatistics struct {
	MinutesPlayed     *string `json:"minutes_played,omitempty"`
	BlocksBroken      *string `json:"blocks_broken,omitempty"`
	MobsDefeated      *string `json:"mobs_defeated,omitempty"`
	DistanceTravelled *string `json:"distance_travelled,omitempty"`
}

// profileStatistics requests the same four title statistics through the Go
// core's authenticated Xbox transport. No auth state crosses the Rust bridge.
func profileStatistics(ctx context.Context, client *http.Client, xuid string) (*ProfileStatistics, error) {
	if _, err := strconv.ParseUint(xuid, 10, 64); err != nil {
		return nil, fmt.Errorf("invalid statistics user ID")
	}
	path := profileStatisticsEndpoint + "/users/xuid(" + xuid + ")/scids/" + auth.ServiceConfigID.String() + "/stats/" + strings.Join(profileStatisticNames[:], ",")
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, path, nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("x-xbl-contract-version", "1")
	req.Header.Set("Accept", "application/json")
	resp, err := client.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		// Error bodies can contain credentials; never include them in errors.
		return nil, fmt.Errorf("statistics service: HTTP %d", resp.StatusCode)
	}
	return decodeProfileStatistics(io.LimitReader(resp.Body, 1<<20), xuid)
}

// decodeProfileStatistics reads the SDK's single-user response and skips odd
// values without manufacturing a zero or discarding other valid statistics.
func decodeProfileStatistics(reader io.Reader, xuid string) (*ProfileStatistics, error) {
	type statistic struct {
		Name  string          `json:"statname"`
		Value json.RawMessage `json:"value"`
	}
	type user struct {
		XUID  string      `json:"xuid"`
		Stats []statistic `json:"stats"`
	}
	var result struct {
		user
		User *user `json:"user"`
	}
	if err := json.NewDecoder(reader).Decode(&result); err != nil {
		return nil, fmt.Errorf("decode statistics: %w", err)
	}
	data := result.user
	if result.User != nil {
		data = *result.User
	}
	if data.XUID != xuid {
		return nil, fmt.Errorf("statistics response user mismatch")
	}
	stats := &ProfileStatistics{}
	fields := []*string{nil, nil, nil, nil}
	for _, entry := range data.Stats {
		value := strings.TrimSpace(string(entry.Value))
		if strings.HasPrefix(value, "\"") {
			if err := json.Unmarshal(entry.Value, &value); err != nil {
				continue
			}
		}
		number, err := strconv.ParseFloat(value, 64)
		if err != nil || math.IsNaN(number) || math.IsInf(number, 0) || number < 0 {
			continue
		}
		for index, name := range profileStatisticNames {
			if entry.Name == name {
				fields[index] = &value
			}
		}
	}
	stats.MinutesPlayed, stats.BlocksBroken, stats.MobsDefeated, stats.DistanceTravelled = fields[0], fields[1], fields[2], fields[3]
	return stats, nil
}
