package catalog

import (
	"cmp"
	"context"
	"fmt"
	"slices"
	"strings"

	"github.com/df-mc/go-xsapi/v2/social"
	"github.com/hashimthearab/rust-mcbe/core/authcache"
)

// MaxPeople bounds the friends the invite screen lists; online friends are kept first.
const MaxPeople = 100

// gamerpicSide is the square size friends' gamerpics are requested at, enough for a list row.
const gamerpicSide = 128

// Person is one Xbox friend as the invite screen lists them. Gamerpic.Path is filled in by a
// caller that caches the artwork.
type Person struct {
	XUID     string `json:"xuid"`
	Gamertag string `json:"gamertag"`
	Online   bool   `json:"online"`
	Gamerpic Image  `json:"gamerpic"`
}

// People lists the signed-in account's Xbox friends, online first and each group by gamertag.
func People(ctx context.Context, account *authcache.Account) ([]Person, error) {
	xbl, err := XboxClient(ctx, account)
	if err != nil {
		return nil, err
	}
	defer xbl.Close()
	return friendPeople(ctx, xbl.Social())
}

// friendPeople reads the decorated friends list, which carries the presence and gamerpic rows show.
func friendPeople(ctx context.Context, people *social.Client) ([]Person, error) {
	users, err := people.People(ctx, social.PeopleListFriends, social.PeopleListConfig{})
	if err != nil {
		return nil, fmt.Errorf("list friends: %w", err)
	}
	result := make([]Person, 0, len(users))
	for _, user := range users {
		gamertag := strings.TrimSpace(user.GamerTag)
		if gamertag == "" {
			gamertag = strings.TrimSpace(user.DisplayName)
		}
		if user.XUID == "" || gamertag == "" {
			continue
		}
		person := Person{XUID: user.XUID, Gamertag: gamertag, Online: strings.EqualFold(user.PresenceState, "Online")}
		if validArtworkURL(user.DisplayPictureRawURL) {
			person.Gamerpic.URL = social.ResizeDisplayPictureURL(user.DisplayPictureRawURL, social.ResizePictureOptions{
				Format: "png", Size: [2]int{gamerpicSide, gamerpicSide},
			})
		}
		result = append(result, person)
	}
	slices.SortStableFunc(result, func(a, b Person) int {
		if a.Online != b.Online {
			if a.Online {
				return -1
			}
			return 1
		}
		return cmp.Compare(strings.ToLower(a.Gamertag), strings.ToLower(b.Gamertag))
	})
	return result[:min(len(result), MaxPeople)], nil
}
