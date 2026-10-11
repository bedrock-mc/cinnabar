package catalog

import (
	"context"
	"math"
	"strconv"

	"github.com/df-mc/go-xsapi/v2"
	"github.com/df-mc/go-xsapi/v2/achievements"
	"github.com/hashimthearab/rust-mcbe/core/internal/locale"
	"github.com/sandertv/gophertunnel/minecraft/auth"
)

// ProfileAchievements is the title-wide summary and achievement data used by Overview.
// Suggested ordering and persona reward art need the game's additional metadata.
type ProfileAchievements struct {
	Unlocked          int                  `json:"unlocked"`
	Total             int                  `json:"total"`
	CurrentGamerscore *int64               `json:"current_gamerscore"`
	MaxGamerscore     *int64               `json:"max_gamerscore"`
	Entries           []ProfileAchievement `json:"entries"`
}

// ProfileAchievement holds the Xbox service fields without inventing game metadata.
type ProfileAchievement struct {
	ID             string `json:"id"`
	Name           string `json:"name"`
	Description    string `json:"description"`
	Image          Image  `json:"image"`
	Gamerscore     *int64 `json:"gamerscore,omitempty"`
	Locked         bool   `json:"locked"`
	DateUnlocked   string `json:"date_unlocked,omitempty"`
	SuggestedOrder *int   `json:"suggested_order,omitempty"`
}

// profileAchievements maps the shared Xbox client results into the Overview summary.
func profileAchievements(ctx context.Context, client *achievements.Client) (*ProfileAchievements, error) {
	values, err := client.All(ctx, achievements.Filter{TitleID: uint32(auth.AndroidConfig.TitleID)}, xsapi.RequestHeader("Accept-Language", locale.Default))
	if err != nil {
		return nil, err
	}
	currentScore, maxScore := int64(0), int64(0)
	result := &ProfileAchievements{Entries: []ProfileAchievement{}, CurrentGamerscore: &currentScore, MaxGamerscore: &maxScore}
	for _, value := range values {
		entry := profileAchievement(value)
		result.Entries = append(result.Entries, entry)
		result.Total++
		if !entry.Locked {
			result.Unlocked++
		}
		if entry.Gamerscore == nil {
			result.MaxGamerscore = nil
			if !entry.Locked {
				result.CurrentGamerscore = nil
			}
		} else {
			addAchievementScore(&result.MaxGamerscore, *entry.Gamerscore)
			if !entry.Locked {
				addAchievementScore(&result.CurrentGamerscore, *entry.Gamerscore)
			}
		}
	}
	return result, nil
}

// profileAchievement maps localized service content and ignores unusable art or rewards.
func profileAchievement(value achievements.Achievement) ProfileAchievement {
	entry := ProfileAchievement{ID: value.ID, Name: value.Name, Description: value.Description, Locked: value.ProgressState != "Achieved"}
	if entry.Locked {
		entry.Description = value.LockedDescription
	} else {
		entry.DateUnlocked = value.Progression.TimeUnlocked
	}
	for _, asset := range value.MediaAssets {
		if asset.Type == "Icon" && validArtworkURL(asset.URL) {
			entry.Image.URL = asset.URL
			break
		}
	}
	for _, reward := range value.Rewards {
		if reward.Type == "Gamerscore" {
			if score, err := strconv.ParseInt(reward.Value, 10, 64); err == nil && score >= 0 {
				entry.Gamerscore = &score
			}
			break
		}
	}
	return entry
}

// addAchievementScore leaves an incomplete or overflowing service total unavailable.
func addAchievementScore(total **int64, score int64) {
	if *total == nil {
		return
	}
	if score > math.MaxInt64-**total {
		*total = nil
		return
	}
	**total += score
}
