package launcher

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/catalog"
	"github.com/hashimthearab/rust-mcbe/core/control"
	"github.com/sandertv/gophertunnel/minecraft/service/gatherings"
)

// TestExperienceCountsKeepMissingSeparateFromZero covers joins by ID and untouched layout state.
func TestExperienceCountsKeepMissingSeparateFromZero(t *testing.T) {
	first, second, missing := uuid.New(), uuid.New(), uuid.New()
	count, zero, negative := int64(12345), int64(0), int64(-1)
	servers := []catalog.FeaturedServer{
		{Address: catalog.GatheringTargetPrefix + first.String()},
		{Address: catalog.GatheringTargetPrefix + second.String()},
		{Address: catalog.GatheringTargetPrefix + missing.String()},
		{Address: "play.example.test"},
		{Address: catalog.GatheringTargetPrefix + "invalid"},
	}
	counts := []gatherings.ExperiencePlayerCount{
		{ExperienceID: second, PlayerCount: &zero},
		{ExperienceID: first, PlayerCount: &count},
		{ExperienceID: first, PlayerCount: &zero},
		{ExperienceID: missing, PlayerCount: &negative},
		{ExperienceID: missing, PlayerCount: &count},
		{ExperienceID: uuid.New(), PlayerCount: &count},
	}
	got := withExperienceCounts(servers, counts)
	if got[0].PlayerCount == nil || *got[0].PlayerCount != count || got[1].PlayerCount == nil || *got[1].PlayerCount != 0 {
		t.Fatal("counts did not follow experience IDs")
	}
	for _, server := range got[2:] {
		if server.PlayerCount != nil {
			t.Fatal("missing count became a number")
		}
	}
	for _, server := range servers {
		if server.PlayerCount != nil {
			t.Fatal("live counts changed the persisted layout")
		}
	}
}

// TestFeaturedCountsRefreshIndependentlyOfLayout keeps live data outside the feed cache.
func TestFeaturedCountsRefreshIndependentlyOfLayout(t *testing.T) {
	id, value := uuid.New(), int64(12)
	layoutCalls, countCalls := 0, 0
	var countErr error
	s := New(Config{
		Account: testAccount(),
		Featured: func(context.Context, *authcache.Account) ([]catalog.FeaturedServer, error) {
			layoutCalls++
			return []catalog.FeaturedServer{{Address: catalog.GatheringTargetPrefix + id.String()}}, nil
		},
		ExperienceCounts: func(context.Context, *authcache.Account) ([]gatherings.ExperiencePlayerCount, error) {
			countCalls++
			return []gatherings.ExperiencePlayerCount{{ExperienceID: id, PlayerCount: &value}}, countErr
		},
	})
	if _, err := s.FeaturedServers(context.Background()); err != nil || countCalls != 0 {
		t.Fatal("ordinary layout request fetched populations")
	}
	for _, next := range []int64{12, 42} {
		value = next
		got, err := s.FeaturedServersWithCounts(context.Background())
		if err != nil || len(got) != 1 || got[0].PlayerCount == nil || *got[0].PlayerCount != next {
			t.Fatalf("featured = %+v, error = %v", got, err)
		}
	}
	countErr = errors.New("offline")
	got, err := s.FeaturedServersWithCounts(context.Background())
	if err != nil || got[0].PlayerCount == nil || *got[0].PlayerCount != value {
		t.Fatal("failed count refresh discarded the typed client's last good value")
	}
	if layoutCalls != 1 || countCalls != 3 {
		t.Fatalf("dispatches: layout=%d count-client=%d", layoutCalls, countCalls)
	}
	if s.snap.Featured.Value[0].PlayerCount != nil {
		t.Fatal("counts entered the disk snapshot")
	}
}

// TestExperienceCountReplyAfterSignOutIsDiscarded prevents a late result crossing account state.
func TestExperienceCountReplyAfterSignOutIsDiscarded(t *testing.T) {
	id := uuid.New()
	var s *Service
	s = New(Config{
		Account: testAccount(),
		ExperienceCounts: func(context.Context, *authcache.Account) ([]gatherings.ExperiencePlayerCount, error) {
			s.signedOut.Store(true)
			return nil, nil
		},
	})
	s.snap.Featured = feed[[]catalog.FeaturedServer]{Fetched: time.Now(), Value: []catalog.FeaturedServer{{Address: catalog.GatheringTargetPrefix + id.String()}}}
	if got, err := s.FeaturedServersWithCounts(context.Background()); !errors.Is(err, control.ErrSignedOut) || got != nil {
		t.Fatalf("late reply = %+v, error = %v", got, err)
	}
}
