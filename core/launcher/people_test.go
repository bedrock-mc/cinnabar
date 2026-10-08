package launcher

import (
	"context"
	"errors"
	"fmt"
	"path/filepath"
	"sync"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/catalog"
	"github.com/hashimthearab/rust-mcbe/core/control"
)

// Every friend's gamerpic is cached once, under its own directory beside the feed art.
func TestPeopleCachesEveryGamerpicApartFromFeedArt(t *testing.T) {
	art := t.TempDir()
	var mu sync.Mutex
	directories := map[string]bool{}
	service := New(Config{
		Account: testAccount(), ArtworkDir: art,
		People: func(context.Context, *authcache.Account) ([]catalog.Person, error) {
			people := make([]catalog.Person, gamerpicFetchers*2+3)
			for index := range people {
				people[index] = catalog.Person{XUID: fmt.Sprint(index + 1), Gamertag: "p", Gamerpic: catalog.Image{URL: fmt.Sprintf("https://a.test/%d", index)}}
			}
			return people, nil
		},
		CacheArt: func(_ context.Context, directory string, images []*catalog.Image) {
			mu.Lock()
			defer mu.Unlock()
			directories[directory] = true
			for _, image := range images {
				if image.Path != "" {
					t.Errorf("%s cached twice", image.URL)
				}
				image.Path = filepath.Join(directory, filepath.Base(image.URL))
			}
		},
	})
	people, err := service.People(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	want := filepath.Join(art, peopleArtDir)
	if len(directories) != 1 || !directories[want] {
		t.Fatalf("cached into %v, want %s", directories, want)
	}
	for index, person := range people {
		if person.Gamerpic.Path != filepath.Join(want, fmt.Sprint(index)) {
			t.Fatalf("person %d gamerpic = %+v", index, person.Gamerpic)
		}
	}
}

func TestPeopleNeedsAnAccount(t *testing.T) {
	if _, err := New(Config{}).People(context.Background()); !errors.Is(err, control.ErrSignedOut) {
		t.Fatalf("People() err = %v", err)
	}
}
