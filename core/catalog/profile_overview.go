package catalog

import (
	_ "embed"
	"sort"
	"strconv"
	"strings"
)

// overviewAchievementLimit is shared with Rust's Overview drawing and art loading.
// Vanilla rules: docs/profile-parity.md.
//
//go:embed profile_overview_limit.txt
var overviewAchievementLimit string

// ProfileOverviewAchievementLimit returns the shared number of visible cards per section.
func ProfileOverviewAchievementLimit() int {
	limit, err := strconv.Atoi(strings.TrimSpace(overviewAchievementLimit))
	if err != nil || limit <= 0 {
		panic("invalid embedded Profile Overview achievement limit")
	}
	return limit
}

// ProfileOverviewAchievements selects only displayed icons, preserving stable ties
// in suggested order and newest-first completion order, just like Rust's Overview.
func ProfileOverviewAchievements(entries []ProfileAchievement) []*ProfileAchievement {
	var sections [2][]*ProfileAchievement
	for index := range entries {
		entry := &entries[index]
		if entry.Locked && entry.SuggestedOrder != nil {
			sections[0] = append(sections[0], entry)
		} else if !entry.Locked && entry.DateUnlocked != "" {
			sections[1] = append(sections[1], entry)
		}
	}
	sort.SliceStable(sections[0], func(i, j int) bool { return *sections[0][i].SuggestedOrder < *sections[0][j].SuggestedOrder })
	sort.SliceStable(sections[1], func(i, j int) bool { return sections[1][i].DateUnlocked > sections[1][j].DateUnlocked })
	limit := ProfileOverviewAchievementLimit()
	for index := range sections {
		if len(sections[index]) > limit {
			sections[index] = sections[index][:limit]
		}
	}
	return append(sections[0], sections[1]...)
}
