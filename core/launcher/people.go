package launcher

import (
	"context"
	"path/filepath"
	"sync"

	"github.com/hashimthearab/rust-mcbe/core/catalog"
)

const (
	// peopleArtDir keeps friends' gamerpics apart from the feed art that pruning manages.
	peopleArtDir = "people"
	// gamerpicFetchers download a long friends list's gamerpics in parallel.
	gamerpicFetchers = 8
)

// People lists the account's Xbox friends with their gamerpics cached.
func (s *Service) People(ctx context.Context) ([]catalog.Person, error) {
	src, err := s.source()
	if err != nil {
		return nil, err
	}
	people, err := s.cfg.People(ctx, src)
	if err != nil || s.cfg.ArtworkDir == "" {
		return people, err
	}
	directory := filepath.Join(s.cfg.ArtworkDir, peopleArtDir)
	var fetchers sync.WaitGroup
	for first := range min(gamerpicFetchers, len(people)) {
		var batch []*catalog.Image
		for index := first; index < len(people); index += gamerpicFetchers {
			batch = append(batch, &people[index].Gamerpic)
		}
		fetchers.Go(func() { s.cfg.CacheArt(ctx, directory, batch) })
	}
	fetchers.Wait()
	return people, nil
}
