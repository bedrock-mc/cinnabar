package catalog

import (
	"context"
	"io"
	"net/http"
	"strconv"
	"strings"
	"testing"

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
	got, err := profileAchievements(context.Background(), client, "123")
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

// TestProfileAchievementsRejectRepeatedContinuation prevents endless service loops.
func TestProfileAchievementsRejectRepeatedContinuation(t *testing.T) {
	client := &http.Client{Transport: roundTripFunc(func(*http.Request) (*http.Response, error) {
		return &http.Response{StatusCode: http.StatusOK, Body: io.NopCloser(strings.NewReader(`{"achievements":[],"pagingInfo":{"continuationToken":"same"}}`))}, nil
	})}
	if _, err := profileAchievements(context.Background(), client, "123"); err == nil {
		t.Fatal("repeated continuation accepted")
	}
}

// TestProfileAchievementsBoundsUniquePages rejects an endless stream of advancing pages.
func TestProfileAchievementsBoundsUniquePages(t *testing.T) {
	calls := 0
	client := &http.Client{Transport: roundTripFunc(func(*http.Request) (*http.Response, error) {
		calls++
		raw := `{"achievements":[],"pagingInfo":{"continuationToken":"` + strconv.Itoa(calls) + `"}}`
		return &http.Response{StatusCode: http.StatusOK, Body: io.NopCloser(strings.NewReader(raw))}, nil
	})}
	if _, err := profileAchievements(context.Background(), client, "123"); err == nil || calls != maxAchievementPages {
		t.Fatalf("pagination result: calls=%d error=%v", calls, err)
	}
}

// TestAchievementTotalsPreserveUnavailableRewards skips odd values without inventing a total.
func TestAchievementTotalsPreserveUnavailableRewards(t *testing.T) {
	value := xboxAchievement{ID: "test", ProgressState: "Achieved"}
	value.Rewards = append(value.Rewards, struct {
		Type  string `json:"type"`
		Value string `json:"value"`
	}{Type: "Gamerscore", Value: "not a number"})
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
