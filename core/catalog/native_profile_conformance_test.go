package catalog

import (
	"context"
	_ "embed"
	"encoding/json"
	"io"
	"net/http"
	"reflect"
	"slices"
	"strings"
	"testing"

	"github.com/df-mc/go-xsapi/v2/achievements"
	"github.com/df-mc/go-xsapi/v2/xal/xsts"
	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/auth"
	"github.com/sandertv/gophertunnel/minecraft/service/userstats"
)

//go:embed native_profile_policy.json
var nativeProfilePolicyJSON []byte

//go:embed native_profile_conformance.json
var nativeProfileConformanceJSON []byte

// TestNativeProfilePolicyMatchesActualGoRequests fences Rust's retail selection against the existing Go owner.
func TestNativeProfilePolicyMatchesActualGoRequests(t *testing.T) {
	var policy struct {
		Schema     int `json:"schema"`
		Statistics struct {
			Configurations []uuid.UUID `json:"configuration_ids"`
			Names          []string    `json:"names"`
		} `json:"statistics"`
		Title uint32 `json:"achievement_title"`
	}
	if err := json.Unmarshal(nativeProfilePolicyJSON, &policy); err != nil {
		t.Fatal(err)
	}
	if policy.Schema != 1 || int64(policy.Title) != auth.AndroidConfig.TitleID {
		t.Fatal("native profile schema or title differs from actual Go policy")
	}
	requests := 0
	client := &http.Client{Transport: roundTripFunc(func(request *http.Request) (*http.Response, error) {
		requests++
		defer request.Body.Close()
		var body struct {
			Users      []string                        `json:"requestedusers"`
			Selections []userstats.RequestedStatistics `json:"requestedscids"`
		}
		if err := json.NewDecoder(request.Body).Decode(&body); err != nil {
			t.Fatal(err)
		}
		if request.Method != http.MethodPost || request.URL.Host != "userstats.xboxlive.com" ||
			!slices.Equal(body.Users, []string{"123"}) || len(body.Selections) != len(policy.Statistics.Configurations) {
			t.Fatal("native profile policy does not match actual batch request")
		}
		for i, selection := range body.Selections {
			if selection.ServiceConfigID != policy.Statistics.Configurations[i] || !slices.Equal(selection.Names, policy.Statistics.Names) {
				t.Fatalf("native statistics selection %d differs from actual Go owner", i)
			}
		}
		return &http.Response{StatusCode: http.StatusOK, Header: make(http.Header), Body: io.NopCloser(strings.NewReader(`{"users":[{"xuid":"123","scids":[]}]}`))}, nil
	})}
	if _, err := profileStatistics(context.Background(), client, "123"); err != nil {
		t.Fatal(err)
	}
	if requests != 1 {
		t.Fatalf("statistics requests = %d", requests)
	}
}

// TestNativeProfileConformance uses the same original response/expectation vectors as the Rust projection.
func TestNativeProfileConformance(t *testing.T) {
	var vectors struct {
		Statistics []struct {
			Name     string            `json:"name"`
			Response json.RawMessage   `json:"response"`
			Expected ProfileStatistics `json:"expected"`
			Error    bool              `json:"error"`
		} `json:"statistics"`
		Achievements []struct {
			Name     string              `json:"name"`
			Response json.RawMessage     `json:"response"`
			Expected ProfileAchievements `json:"expected"`
		} `json:"achievements"`
	}
	if err := json.Unmarshal(nativeProfileConformanceJSON, &vectors); err != nil {
		t.Fatal(err)
	}
	for _, vector := range vectors.Statistics {
		t.Run(vector.Name, func(t *testing.T) {
			client := profileResponseClient(vector.Response)
			got, err := profileStatistics(context.Background(), client, "123")
			if vector.Error {
				if err == nil {
					t.Fatal("wrong subject was accepted")
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			if !reflect.DeepEqual(got, &vector.Expected) {
				t.Fatalf("statistics = %+v, want %+v", got, vector.Expected)
			}
		})
	}
	for _, vector := range vectors.Achievements {
		t.Run(vector.Name, func(t *testing.T) {
			client := achievements.New(profileResponseClient(vector.Response), xsts.UserInfo{XUID: "123"})
			got, err := profileAchievements(context.Background(), client)
			if err != nil {
				t.Fatal(err)
			}
			if !reflect.DeepEqual(got, &vector.Expected) {
				t.Fatalf("achievements = %+v, want %+v", got, vector.Expected)
			}
		})
	}
}

// profileResponseClient gives the actual service/projection owner one original bounded response.
func profileResponseClient(body []byte) *http.Client {
	return &http.Client{Transport: roundTripFunc(func(*http.Request) (*http.Response, error) {
		return &http.Response{StatusCode: http.StatusOK, Header: make(http.Header), Body: io.NopCloser(strings.NewReader(string(body)))}, nil
	})}
}
