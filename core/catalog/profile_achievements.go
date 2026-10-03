package catalog

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"math"
	"net/http"
	"net/url"
	"strconv"

	"github.com/hashimthearab/rust-mcbe/core/internal/locale"
	"github.com/sandertv/gophertunnel/minecraft/auth"
)

const profileAchievementsEndpoint = "https://achievements.xboxlive.com"

// Resource bounds prevent an inconsistent service from returning unbounded pages.
const maxAchievementPages = 128
const maxProfileAchievements = 65536

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

type xboxAchievement struct {
	ID                string `json:"id"`
	Name              string `json:"name"`
	Description       string `json:"description"`
	LockedDescription string `json:"lockedDescription"`
	ProgressState     string `json:"progressState"`
	Progression       struct {
		TimeUnlocked string `json:"timeUnlocked"`
	} `json:"progression"`
	MediaAssets []struct {
		Type string `json:"type"`
		URL  string `json:"url"`
	} `json:"mediaAssets"`
	Rewards []struct {
		Type  string `json:"type"`
		Value string `json:"value"`
	} `json:"rewards"`
}

// profileAchievements reads every page through the core's Xbox transport. The
// REST shape is the Xbox SDK getAchievementsForTitleId v2 contract, also documented
// at https://learn.microsoft.com/en-us/gaming/gdk/docs/reference/live/rest/uri/achievements/uri-achievementsusersxuidachievementsgetv2.
func profileAchievements(ctx context.Context, client *http.Client, xuid string) (*ProfileAchievements, error) {
	if _, err := strconv.ParseUint(xuid, 10, 64); err != nil {
		return nil, fmt.Errorf("invalid achievement user ID")
	}
	currentScore, maxScore := int64(0), int64(0)
	result := &ProfileAchievements{Entries: []ProfileAchievement{}, CurrentGamerscore: &currentScore, MaxGamerscore: &maxScore}
	seen := map[string]bool{}
	continuation := ""
	for pageIndex := 0; pageIndex < maxAchievementPages; pageIndex++ {
		query := url.Values{"titleId": {strconv.FormatInt(auth.AndroidConfig.TitleID, 10)}}
		if continuation != "" {
			query.Set("continuationToken", continuation)
		}
		request, err := http.NewRequestWithContext(ctx, http.MethodGet, profileAchievementsEndpoint+"/users/xuid("+xuid+")/achievements?"+query.Encode(), nil)
		if err != nil {
			return nil, err
		}
		request.Header.Set("x-xbl-contract-version", "2")
		request.Header.Set("Accept", "application/json")
		request.Header.Set("Accept-Language", locale.Default)
		response, err := client.Do(request)
		if err != nil {
			return nil, err
		}
		var page struct {
			Achievements []xboxAchievement `json:"achievements"`
			PagingInfo   struct {
				ContinuationToken string `json:"continuationToken"`
			} `json:"pagingInfo"`
		}
		if response.StatusCode != http.StatusOK {
			_ = response.Body.Close()
			return nil, fmt.Errorf("achievements service: HTTP %d", response.StatusCode)
		}
		err = json.NewDecoder(io.LimitReader(response.Body, 4<<20)).Decode(&page)
		_ = response.Body.Close()
		if err != nil {
			return nil, fmt.Errorf("decode achievements: %w", err)
		}
		if page.Achievements == nil {
			return nil, fmt.Errorf("achievement response has no collection")
		}
		for _, value := range page.Achievements {
			if value.ID == "" || seen[value.ID] {
				continue
			}
			if len(result.Entries) == maxProfileAchievements {
				return nil, fmt.Errorf("achievement collection exceeds limit")
			}
			seen[value.ID] = true
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
		next := page.PagingInfo.ContinuationToken
		if next == "" {
			return result, nil
		}
		if next == continuation || seen["page:"+next] {
			return nil, fmt.Errorf("achievement continuation did not advance")
		}
		seen["page:"+next] = true
		continuation = next
	}
	return nil, fmt.Errorf("achievement pagination exceeds limit")
}

// profileAchievement maps localized service content and ignores unusable art or rewards.
func profileAchievement(value xboxAchievement) ProfileAchievement {
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
