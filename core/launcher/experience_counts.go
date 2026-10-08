package launcher

import (
	"strings"

	"github.com/google/uuid"
	"github.com/hashimthearab/rust-mcbe/core/catalog"
	"github.com/sandertv/gophertunnel/minecraft/service/gatherings"
)

// hasExperiences avoids requesting counts when the menu has no experience destinations.
func hasExperiences(servers []catalog.FeaturedServer) bool {
	for _, server := range servers {
		if _, ok := experienceID(server.Address); ok {
			return true
		}
	}
	return false
}

// experienceID reads the existing launcher destination without accepting ordinary servers.
func experienceID(address string) (uuid.UUID, bool) {
	raw, ok := strings.CutPrefix(address, catalog.GatheringTargetPrefix)
	if !ok {
		return uuid.Nil, false
	}
	id, err := uuid.Parse(raw)
	return id, err == nil && id != uuid.Nil
}

// withExperienceCounts overlays live counts without mutating or persisting the layout cache.
// The first entry for an experience wins, including an unavailable count.
func withExperienceCounts(servers []catalog.FeaturedServer, counts []gatherings.ExperiencePlayerCount) []catalog.FeaturedServer {
	byID := make(map[uuid.UUID]*int64, len(counts))
	for _, count := range counts {
		if _, seen := byID[count.ExperienceID]; seen {
			continue
		}
		byID[count.ExperienceID] = nil
		if count.PlayerCount != nil && *count.PlayerCount >= 0 {
			byID[count.ExperienceID] = count.PlayerCount
		}
	}
	result := append([]catalog.FeaturedServer(nil), servers...)
	for i := range result {
		id, ok := experienceID(result[i].Address)
		result[i].PlayerCount = nil
		if ok {
			result[i].PlayerCount = byID[id]
		}
	}
	return result
}
