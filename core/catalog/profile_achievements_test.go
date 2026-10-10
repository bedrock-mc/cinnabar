package catalog

import (
	"context"
	"encoding/json"
	"io"
	"math"
	"net/http"
	"strconv"
	"strings"
	"testing"

	"github.com/df-mc/go-xsapi/v2/achievements"
	"github.com/df-mc/go-xsapi/v2/xal/xsts"
	"github.com/sandertv/gophertunnel/minecraft/auth"
)

// TestProfileAchievementsPages uses authored SDK responses to verify title scope and summaries.
func TestProfileAchievementsPages(t *testing.T) {
	calls := 0
	client := &http.Client{Transport: roundTripFunc(func(request *http.Request) (*http.Response, error) {
		calls++
		if request.Method != http.MethodGet || request.URL.Path != "/users/xuid(123)/achievements" || request.URL.Query().Get("titleId") != strconv.FormatInt(auth.AndroidConfig.TitleID, 10) || request.Header.Get("x-xbl-contract-version") != "2" {
			t.Fatalf("unexpected request: %s %s", request.Method, request.URL)
		}
		raw := `{"achievements":[{"id":"1","name":"Open inventory","description":"Unlocked","lockedDescription":"Open it","progressState":"Achieved","progression":{"timeUnlocked":"2026-01-02T00:00:00Z"},"mediaAssets":[{"type":"Icon","url":"https://fixture.test/one.png"}],"rewards":[{"type":"Gamerscore","value":"10"}]}],"pagingInfo":{"continuationToken":"next"}}`
		if calls == 2 {
			if request.URL.Query().Get("continuationToken") != "next" {
				t.Fatal("missing continuation")
			}
			raw = `{"achievements":[{"id":"2","name":"Mine","lockedDescription":"Break a block","progressState":"NotStarted","rewards":[{"type":"Gamerscore","value":"20"}]},{"id":"1"}],"pagingInfo":{"continuationToken":null}}`
		}
		return &http.Response{StatusCode: http.StatusOK, Body: io.NopCloser(strings.NewReader(raw))}, nil
	})}
	got, err := profileAchievements(context.Background(), achievements.New(client, xsts.UserInfo{XUID: "123"}))
	if err != nil {
		t.Fatal(err)
	}
	if calls != 2 || got.Total != 2 || got.Unlocked != 1 || got.CurrentGamerscore == nil || *got.CurrentGamerscore != 10 || got.MaxGamerscore == nil || *got.MaxGamerscore != 30 {
		t.Fatalf("summary = %+v, calls = %d", got, calls)
	}
	if got.Entries[1].Description != "Break a block" || !got.Entries[1].Locked || got.Entries[1].SuggestedOrder != nil {
		t.Fatalf("locked card = %+v", got.Entries[1])
	}
	if got.Entries[0].DateUnlocked != "2026-01-02T00:00:00Z" || got.Entries[0].Image.URL != "https://fixture.test/one.png" {
		t.Fatalf("completed card = %+v", got.Entries[0])
	}
}

// TestProfileAchievementUnavailableScores keeps incomplete and overflowing totals
// unavailable while allowing a known current score when only locked rewards are missing.
func TestProfileAchievementUnavailableScores(t *testing.T) {
	for _, test := range []struct {
		name        string
		state       string
		rewards     []achievements.Reward
		wantCurrent bool
	}{
		{name: "missing unlocked reward", state: "Achieved"},
		{name: "missing locked reward", state: "NotStarted", wantCurrent: true},
		{name: "invalid reward", state: "Achieved", rewards: []achievements.Reward{{Type: "Gamerscore", Value: "invalid"}}},
		{name: "negative reward", state: "Achieved", rewards: []achievements.Reward{{Type: "Gamerscore", Value: "-1"}}},
		{name: "reward overflow", state: "Achieved", rewards: []achievements.Reward{{Type: "Gamerscore", Value: "9223372036854775808"}}},
		{name: "total overflow", state: "Achieved", rewards: []achievements.Reward{{Type: "Gamerscore", Value: strconv.FormatInt(math.MaxInt64, 10)}}},
	} {
		t.Run(test.name, func(t *testing.T) {
			body, err := json.Marshal(achievements.Page{Achievements: []achievements.Achievement{
				{ID: "one", ProgressState: "Achieved", Rewards: []achievements.Reward{{Type: "Gamerscore", Value: "10"}}},
				{ID: "two", ProgressState: test.state, Rewards: test.rewards},
				{ID: "three", ProgressState: "Achieved", Rewards: []achievements.Reward{{Type: "Gamerscore", Value: "5"}}},
			}})
			if err != nil {
				t.Fatal(err)
			}
			client := &http.Client{Transport: roundTripFunc(func(*http.Request) (*http.Response, error) {
				return &http.Response{StatusCode: http.StatusOK, Body: io.NopCloser(strings.NewReader(string(body)))}, nil
			})}
			got, err := profileAchievements(context.Background(), achievements.New(client, xsts.UserInfo{XUID: "123"}))
			if err != nil {
				t.Fatal(err)
			}
			if got.Total != 3 || got.MaxGamerscore != nil || (got.CurrentGamerscore != nil) != test.wantCurrent {
				t.Fatalf("summary = %+v", got)
			}
			if test.wantCurrent && *got.CurrentGamerscore != 15 {
				t.Fatalf("current score = %d, want 15", *got.CurrentGamerscore)
			}
		})
	}
}

// TestAchievementTotalsPreserveUnavailableRewards skips odd values without inventing a total.
func TestAchievementTotalsPreserveUnavailableRewards(t *testing.T) {
	value := achievements.Achievement{ID: "test", ProgressState: "Achieved"}
	value.Rewards = append(value.Rewards, achievements.Reward{Type: "Gamerscore", Value: "not a number"})
	entry := profileAchievement(value)
	if entry.Gamerscore != nil {
		t.Fatal("invalid gamerscore was made numeric")
	}
	score := int64(9223372036854775807)
	total := &score
	addAchievementScore(&total, 1)
	if total != nil {
		t.Fatal("overflowed total remained available")
	}
}
