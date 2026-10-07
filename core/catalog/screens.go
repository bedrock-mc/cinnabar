package catalog

import (
	"context"
	"errors"
	"fmt"
	"strings"

	"github.com/df-mc/go-xsapi/v2/social"
	"github.com/google/uuid"
	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"github.com/sandertv/gophertunnel/minecraft/service/gatherings"
)

// FeaturedServer is a featured server with the details the play screen's
// server info panel shows. Image fields are HTTPS URLs; paths are filled in by
// a caller that caches the artwork.
type FeaturedServer struct {
	Name         string   `json:"name"`
	Group        string   `json:"group"`
	PlayerCount  *int64   `json:"player_count,omitempty"`
	Address      string   `json:"address"`
	Caption      string   `json:"caption"`
	Description  string   `json:"description,omitempty"`
	NewsTitle    string   `json:"news_title,omitempty"`
	News         string   `json:"news,omitempty"`
	Logo         Image    `json:"logo"`
	Background   Image    `json:"background"` // the details panel's banner
	Screenshots  []Image  `json:"screenshots"`
	Games        []Game   `json:"games"`
	Tags         []string `json:"tags,omitempty"`
	thumbnailURL string   // compact catalog artwork may use a game image instead of the logo
}

// Game is one activity a featured server or gathering advertises.
type Game struct {
	Title       string `json:"title"`
	Subtitle    string `json:"subtitle,omitempty"`
	Description string `json:"description,omitempty"`
	Image       Image  `json:"image"`
}

// Image is remote HTTPS artwork plus its locally cached copy, when one exists.
type Image struct {
	URL  string `json:"url,omitempty"`
	Path string `json:"path,omitempty"`
}

// Profile is the signed-in account as the start and profile screens show it. A count whose
// lookup failed is omitted rather than reported as zero.
type Profile struct {
	Gamertag                string               `json:"gamertag"`
	XUID                    string               `json:"xuid"`
	Gamerpic                Image                `json:"gamerpic"`
	Avatar                  Image                `json:"avatar"`
	AvatarError             bool                 `json:"avatar_error,omitempty"`
	FeaturedScreenshot      Image                `json:"featured_screenshot"`
	FeaturedScreenshotError bool                 `json:"featured_screenshot_error,omitempty"`
	RealName                string               `json:"real_name,omitempty"`
	PresenceText            string               `json:"presence_text,omitempty"`
	Gamerscore              *int64               `json:"gamerscore,omitempty"`
	Friends                 *int                 `json:"friends,omitempty"`
	Followers               *int                 `json:"followers,omitempty"`
	Statistics              *ProfileStatistics   `json:"statistics,omitempty"`
	Achievements            *ProfileAchievements `json:"achievements,omitempty"`
	partial                 error
}

// Partial returns why lookups failed; their fields are left unset. Callers redact it before logging.
func (p Profile) Partial() error { return p.partial }

// JoinGathering joins the experience now and returns its typed server assignment.
func JoinGathering(ctx context.Context, account *authcache.Account, id uuid.UUID) (*gatherings.Address, error) {
	var address *gatherings.Address
	err := withGatherings(ctx, account, func(client *gatherings.Client) (err error) {
		address, err = client.JoinExperience(ctx, id)
		return err
	})
	return address, err
}

// AccountProfile returns the signed-in gamertag, XUID and gamerpic; a missing
// gamerpic is not an error.
func AccountProfile(ctx context.Context, account *authcache.Account) (Profile, error) {
	finish := ObserveProfileRequest(ctx, "xbox_auth")
	xbl, err := XboxClient(ctx, account)
	finish(err)
	if err != nil {
		return Profile{}, err
	}
	defer xbl.Close()
	info := xbl.UserInfo()
	profile := Profile{Gamertag: info.GamerTag, XUID: info.XUID}
	people := xbl.Social()
	var failures []error
	finish = ObserveProfileRequest(ctx, "identity")
	user, err := people.UserByXUID(ctx, info.XUID)
	finish(err)
	if err != nil {
		failures = append(failures, fmt.Errorf("profile: %w", err))
	} else {
		if validArtworkURL(user.DisplayPictureRawURL) {
			profile.Gamerpic.URL = user.DisplayPictureRawURL
		}
		if profile.Gamertag == "" {
			profile.Gamertag = strings.TrimSpace(user.GamerTag)
		}
		profile.RealName = strings.TrimSpace(user.RealName)
		profile.PresenceText = strings.TrimSpace(user.PresenceText)
		if score, err := user.GamerScore.Int64(); err == nil && score >= 0 {
			profile.Gamerscore = &score
		}
	}
	finish = ObserveProfileRequest(ctx, "friends")
	friends, err := peopleCount(ctx, people, social.PeopleListFriends)
	finish(err)
	if err != nil {
		failures = append(failures, fmt.Errorf("friends: %w", err))
	} else {
		profile.Friends = &friends
	}
	finish = ObserveProfileRequest(ctx, "followers")
	followers, err := peopleCount(ctx, people, social.PeopleListFollowers)
	finish(err)
	if err != nil {
		failures = append(failures, fmt.Errorf("followers: %w", err))
	} else {
		profile.Followers = &followers
	}
	finish = ObserveProfileRequest(ctx, "statistics")
	stats, err := profileStatistics(ctx, xbl.HTTPClient(), info.XUID)
	finish(err)
	if err != nil {
		failures = append(failures, fmt.Errorf("statistics: %w", err))
	} else {
		profile.Statistics = stats
	}
	finish = ObserveProfileRequest(ctx, "achievements")
	achievements, err := profileAchievements(ctx, xbl.HTTPClient(), info.XUID)
	finish(err)
	if err != nil {
		failures = append(failures, fmt.Errorf("achievements: %w", err))
	} else {
		profile.Achievements = achievements
	}
	profile.partial = errors.Join(failures...)
	return profile, nil
}

// peopleCount counts one people list; the game requests these lists without decorations.
func peopleCount(ctx context.Context, people *social.Client, list social.PeopleList) (int, error) {
	users, err := people.People(ctx, list, social.PeopleListConfig{Undecorated: true})
	return len(users), err
}

// withGatherings hands run a gatherings client on the discovered endpoint and the account's token.
func withGatherings(ctx context.Context, account *authcache.Account, run func(*gatherings.Client) error) error {
	if account == nil {
		return errNoAccount
	}
	discovery, err := service.Default(ctx)
	if err != nil {
		return fmt.Errorf("discover services: %w", err)
	}
	client, err := gatheringsClient(discovery, account)
	if err != nil {
		return err
	}
	return run(client)
}

// gatheringsClient builds the gatherings client on discovery's endpoint; there is no fallback host.
func gatheringsClient(discovery *service.Discovery, tokens service.TokenSource) (*gatherings.Client, error) {
	env := new(gatherings.Environment)
	if err := discovery.Environment(env); err != nil {
		return nil, fmt.Errorf("resolve gatherings service: %w", err)
	}
	return env.New(tokens), nil
}

// maxCachedArtwork bounds the artwork directory; the least recently used files go first.
const maxCachedArtwork = 256

// CacheImages downloads each image into directory and fills its path; a
// failed download leaves the path empty.
func CacheImages(ctx context.Context, directory string, images []*Image) {
	if len(images) == 0 {
		return
	}
	cache := artworkCache(directory)
	for _, image := range images {
		if image == nil || image.URL == "" {
			continue
		}
		if cached, err := cache.Fetch(ctx, image.URL); err == nil {
			image.Path = cached.Path
		}
	}
	cache.Prune()
}

// FeaturedImages lists the artwork of servers for CacheImages.
func FeaturedImages(servers []FeaturedServer) []*Image {
	var images []*Image
	for index := range servers {
		server := &servers[index]
		images = append(images, &server.Logo, &server.Background)
		for shot := range server.Screenshots {
			images = append(images, &server.Screenshots[shot])
		}
		for game := range server.Games {
			images = append(images, &server.Games[game].Image)
		}
	}
	return images
}
