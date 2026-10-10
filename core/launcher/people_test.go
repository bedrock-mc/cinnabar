package launcher

import (
	"context"
	"errors"
	"os"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/catalog"
	"github.com/hashimthearab/rust-mcbe/core/control"
)

// People passes gamerpic URLs through untouched; the client downloads them.
func TestPeopleLeavesGamerpicsToTheClient(t *testing.T) {
	art := t.TempDir()
	service := New(Config{
		Account: testAccount(), ArtworkDir: art,
		People: func(context.Context, *authcache.Account) ([]catalog.Person, error) {
			return []catalog.Person{{XUID: "1", Gamertag: "p", Gamerpic: catalog.Image{URL: "https://a.test/1"}}}, nil
		},
	})
	people, err := service.People(context.Background())
	if err != nil || len(people) != 1 || people[0].Gamerpic != (catalog.Image{URL: "https://a.test/1"}) {
		t.Fatalf("people = %+v, err = %v", people, err)
	}
	if entries, _ := os.ReadDir(art); len(entries) != 0 {
		t.Fatalf("core wrote artwork: %v", entries)
	}
}

func TestPeopleNeedsAnAccount(t *testing.T) {
	if _, err := New(Config{}).People(context.Background()); !errors.Is(err, control.ErrSignedOut) {
		t.Fatalf("People() err = %v", err)
	}
}
