package catalog

import (
	"context"
	"errors"
	"fmt"
	"strings"

	playfabcatalog "github.com/df-mc/go-playfab/v2/catalog"
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
	Address      string   `json:"address"`
	Caption      string   `json:"caption"`
	Description  string   `json:"description,omitempty"`
	NewsTitle    string   `json:"news_title,omitempty"`
	News         string   `json:"news,omitempty"`
	Logo         Image    `json:"logo"`
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

// Gathering is a community experience; it is joined by ID only when the player connects.
type Gathering struct {
	ID          string `json:"id"`
	Name        string `json:"name"`
	Caption     string `json:"caption"`
	Description string `json:"description,omitempty"`
	Creator     string `json:"creator,omitempty"`
	Image       Image  `json:"image"`
	StartUnix   int64  `json:"start_unix,omitempty"`
	EndUnix     int64  `json:"end_unix,omitempty"`
}

// Profile is the signed-in account as the start and profile screens show it. A count whose
// lookup failed is omitted rather than reported as zero.
type Profile struct {
	Gamertag     string `json:"gamertag"`
	XUID         string `json:"xuid"`
	Gamerpic     Image  `json:"gamerpic"`
	RealName     string `json:"real_name,omitempty"`
	PresenceText string `json:"presence_text,omitempty"`
	Gamerscore   *int64 `json:"gamerscore,omitempty"`
	Friends      *int   `json:"friends,omitempty"`
	Followers    *int   `json:"followers,omitempty"`
	partial      error
}

// Partial returns why lookups failed; their fields are left unset. Callers redact it before logging.
func (p Profile) Partial() error { return p.partial }

// FeaturedServers lists the featured servers from the gatherings service.
func FeaturedServers(ctx context.Context, account *authcache.Account) ([]FeaturedServer, error) {
	var result []FeaturedServer
	err := withGatherings(ctx, account, func(client *gatherings.Client) error {
		values, err := client.FeaturedServers(ctx)
		if err != nil {
			return err
		}
		result = featuredServers(values)
		return nil
	})
	return result, err
}

// Gatherings lists the community experiences; listing never joins one.
func Gatherings(ctx context.Context, account *authcache.Account) ([]Gathering, error) {
	var result []Gathering
	err := withGatherings(ctx, account, func(client *gatherings.Client) error {
		values, err := client.Experiences(ctx)
		if err != nil {
			return err
		}
		result = make([]Gathering, 0, len(values))
		for _, experience := range values {
			if experience != nil && experience.Valid() {
				result = append(result, gathering(experience))
			}
		}
		return nil
	})
	return result, err
}

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
	xbl, err := XboxClient(ctx, account)
	if err != nil {
		return Profile{}, err
	}
	defer xbl.Close()
	info := xbl.UserInfo()
	profile := Profile{Gamertag: info.GamerTag, XUID: info.XUID}
	social := xbl.Social()
	var failures []error
	if user, err := social.UserByXUID(ctx, info.XUID); err != nil {
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
	if friends, err := social.Friends(ctx); err != nil {
		failures = append(failures, fmt.Errorf("friends: %w", err))
	} else {
		count := len(friends)
		profile.Friends = &count
	}
	if followers, err := social.Followers(ctx); err != nil {
		failures = append(failures, fmt.Errorf("followers: %w", err))
	} else {
		count := len(followers)
		profile.Followers = &count
	}
	profile.partial = errors.Join(failures...)
	return profile, nil
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

func featuredServers(values []*gatherings.FeaturedServer) []FeaturedServer {
	result := make([]FeaturedServer, 0, len(values))
	for _, server := range values {
		if server == nil || !server.Valid() {
			continue
		}
		result = append(result, FeaturedServer{
			Name:         displayName(server.Item.Title.Neutral(), server.CreatorName, "Featured server"),
			Address:      server.Address(),
			Caption:      firstGameCaption(server.AvailableGames, "Featured server"),
			Description:  strings.TrimSpace(server.Item.Description.Neutral()),
			NewsTitle:    strings.TrimSpace(server.NewsTitle),
			News:         strings.TrimSpace(server.News),
			Logo:         Image{URL: artworkURL(server.Item, nil)},
			Screenshots:  screenshots(server.Item),
			Games:        games(server.Item, server.AvailableGames),
			Tags:         server.Item.Tags,
			thumbnailURL: artworkURL(server.Item, server.AvailableGames),
		})
	}
	return result
}

func gathering(experience *gatherings.Experience) Gathering {
	entry := Gathering{
		ID:          experience.ID.String(),
		Name:        displayName(experience.Item.Title.Neutral(), experience.CreatorName, "Gathering"),
		Caption:     firstGameCaption(experience.AvailableGames, "Community gathering"),
		Description: strings.TrimSpace(experience.Item.Description.Neutral()),
		Creator:     strings.TrimSpace(experience.CreatorName),
		Image:       Image{URL: artworkURL(experience.Item, experience.AvailableGames)},
	}
	if !experience.Item.StartDate.IsZero() {
		entry.StartUnix = experience.Item.StartDate.Unix()
	}
	if !experience.Item.EndDate.IsZero() {
		entry.EndUnix = experience.Item.EndDate.Unix()
	}
	return entry
}

func screenshots(item playfabcatalog.Item) []Image {
	result := []Image{}
	for _, image := range item.Images {
		if strings.EqualFold(image.Type, playfabcatalog.ImageTypeScreenshot) && validArtworkURL(image.URL) {
			result = append(result, Image{URL: image.URL})
		}
	}
	return result
}

func games(item playfabcatalog.Item, values []gatherings.AvailableGame) []Game {
	result := make([]Game, 0, len(values))
	for _, value := range values {
		game := Game{
			Title:       strings.TrimSpace(value.Title),
			Subtitle:    strings.TrimSpace(value.Subtitle),
			Description: strings.TrimSpace(value.Description),
		}
		for _, image := range item.Images {
			if value.ImageTag != "" && image.Tag == value.ImageTag && validArtworkURL(image.URL) {
				game.Image.URL = image.URL
				break
			}
		}
		if game.Title != "" || game.Subtitle != "" {
			result = append(result, game)
		}
	}
	return result
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
		images = append(images, &server.Logo)
		for shot := range server.Screenshots {
			images = append(images, &server.Screenshots[shot])
		}
		for game := range server.Games {
			images = append(images, &server.Games[game].Image)
		}
	}
	return images
}

// GatheringImages lists the artwork of gatherings for CacheImages.
func GatheringImages(gatherings []Gathering) []*Image {
	images := make([]*Image, 0, len(gatherings))
	for index := range gatherings {
		images = append(images, &gatherings[index].Image)
	}
	return images
}
