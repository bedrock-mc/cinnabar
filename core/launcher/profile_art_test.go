package launcher

import (
	"context"
	"fmt"
	"reflect"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/catalog"
)

// TestProfileCachesOnlyVisibleAchievementArt keeps hidden icons off the profile's critical path.
func TestProfileCachesOnlyVisibleAchievementArt(t *testing.T) {
	entries := []catalog.ProfileAchievement{}
	for index := 9; index >= 0; index-- {
		order := index
		entries = append(entries,
			catalog.ProfileAchievement{ID: fmt.Sprintf("suggested-%d", index), Locked: true, SuggestedOrder: &order, Image: catalog.Image{URL: fmt.Sprintf("suggested-%d", index)}},
			catalog.ProfileAchievement{ID: fmt.Sprintf("completed-%d", index), DateUnlocked: fmt.Sprintf("2026-01-%02dT00:00:00Z", index+1), Image: catalog.Image{URL: fmt.Sprintf("completed-%d", index)}},
		)
	}
	entries = append(entries, catalog.ProfileAchievement{Locked: true, Image: catalog.Image{URL: "no-suggestion-order"}}, catalog.ProfileAchievement{Image: catalog.Image{URL: "no-completion-date"}})
	var cached []string
	service := New(Config{
		Account: testAccount(), ArtworkDir: t.TempDir(),
		Profile: func(context.Context, *authcache.Account) (catalog.Profile, error) {
			return catalog.Profile{Achievements: &catalog.ProfileAchievements{Entries: entries}}, nil
		},
		CacheArt: func(_ context.Context, _ string, images []*catalog.Image) {
			for _, image := range images {
				if image.URL != "" {
					cached = append(cached, image.URL)
					image.Path = image.URL + ".img"
				}
			}
		},
	})
	profile, err := service.Profile(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	var want []string
	for index := 0; index < catalog.ProfileOverviewAchievementLimit(); index++ {
		want = append(want, fmt.Sprintf("suggested-%d", index))
	}
	for index := 0; index < catalog.ProfileOverviewAchievementLimit(); index++ {
		want = append(want, fmt.Sprintf("completed-%d", 9-index))
	}
	if !reflect.DeepEqual(cached, want) {
		t.Fatalf("cached icons = %v, want displayed icons %v", cached, want)
	}
	if len(profile.Achievements.Entries) != len(entries) {
		t.Fatal("hidden achievement metadata was dropped")
	}
	for _, entry := range profile.Achievements.Entries {
		visible := false
		for _, url := range want {
			visible = visible || entry.Image.URL == url
		}
		if (entry.Image.Path != "") != visible {
			t.Fatalf("wrong cached path for %q: %q", entry.ID, entry.Image.Path)
		}
	}
}
