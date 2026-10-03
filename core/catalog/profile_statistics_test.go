package catalog

import (
	"context"
	"io"
	"net/http"
	"strings"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft/auth"
)

// TestProfileStatisticsRequest uses an authored SDK fixture with no account or network.
func TestProfileStatisticsRequest(t *testing.T) {
	client := &http.Client{Transport: roundTripFunc(func(req *http.Request) (*http.Response, error) {
		want := "/users/xuid(123)/scids/" + auth.ServiceConfigID.String() + "/stats/" + strings.Join(profileStatisticNames[:], ",")
		if req.Method != http.MethodGet || req.URL.Host != strings.TrimPrefix(profileStatisticsEndpoint, "https://") || req.URL.Path != want || req.URL.RawQuery != "" || req.Body != nil {
			t.Fatalf("unexpected statistics request: %s %s", req.Method, req.URL)
		}
		if req.Header.Get("x-xbl-contract-version") != "1" {
			t.Fatal("wrong statistics contract version")
		}
		return &http.Response{StatusCode: http.StatusOK, Header: http.Header{}, Body: io.NopCloser(strings.NewReader(`{"xuid":"123","stats":[{"statname":"MinutesPlayed","type":"Double","value":"120.5"},{"statname":"BlockBrokenTotal","type":"Integer","value":0},{"statname":"MobKilled.IsMonster.1","value":"17"},{"statname":"DistanceTravelled","value":"12345.75"}]}`))}, nil
	})}
	got, err := profileStatistics(context.Background(), client, "123")
	if err != nil {
		t.Fatal(err)
	}
	if got.MinutesPlayed == nil || *got.MinutesPlayed != "120.5" || got.BlocksBroken == nil || *got.BlocksBroken != "0" || got.MobsDefeated == nil || *got.MobsDefeated != "17" || got.DistanceTravelled == nil || *got.DistanceTravelled != "12345.75" {
		t.Fatalf("statistics = %+v", got)
	}
}

// TestProfileStatisticsUnavailable checks missing, malformed and non-finite values.
func TestProfileStatisticsUnavailable(t *testing.T) {
	for _, fixture := range []string{`{"user":{"xuid":"123","stats":[]}}`, `{"xuid":"123","stats":[{"statname":"MinutesPlayed","value":"NaN"},{"statname":"BlockBrokenTotal","value":-1},{"statname":"DistanceTravelled","value":"Infinity"},{"statname":"MobKilled.IsMonster.1","value":{}}]}`} {
		got, err := decodeProfileStatistics(strings.NewReader(fixture), "123")
		if err != nil {
			t.Fatal(err)
		}
		if got.MinutesPlayed != nil || got.BlocksBroken != nil || got.MobsDefeated != nil || got.DistanceTravelled != nil {
			t.Fatal("unavailable statistics fabricated values")
		}
	}
	for _, fixture := range []string{`{"xuid":"999","stats":[]}`, `{"xuid":`} {
		if _, err := decodeProfileStatistics(strings.NewReader(fixture), "123"); err == nil {
			t.Fatal("invalid response accepted")
		}
	}
}

// TestProfileStatisticsFailureDoesNotExposeBody keeps service errors free of body contents.
func TestProfileStatisticsFailureDoesNotExposeBody(t *testing.T) {
	client := &http.Client{Transport: roundTripFunc(func(*http.Request) (*http.Response, error) {
		return &http.Response{StatusCode: http.StatusForbidden, Body: io.NopCloser(strings.NewReader("sensitive body"))}, nil
	})}
	got, err := profileStatistics(context.Background(), client, "123")
	if got != nil || err == nil || strings.Contains(err.Error(), "sensitive") {
		t.Fatalf("failure = %v", err)
	}
}
