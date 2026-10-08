package catalog

import (
	"context"
	"encoding/json"
	"fmt"
	"math/rand/v2"
	"net"
	"slices"
	"strconv"
	"strings"

	playfab "github.com/df-mc/go-playfab/v2/catalog"
	"github.com/google/uuid"
	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"github.com/sandertv/gophertunnel/minecraft/service/gatherings"
)

// FeaturedServers loads the classic Servers catalog, including ranked experiences and creators.
func FeaturedServers(ctx context.Context, account *authcache.Account) ([]FeaturedServer, error) {
	if account == nil {
		return nil, errNoAccount
	}
	discovery, err := service.Default(ctx)
	if err != nil {
		return nil, fmt.Errorf("discover services: %w", err)
	}
	env := new(gatherings.Environment)
	if err := discovery.Environment(env); err != nil {
		return nil, fmt.Errorf("resolve gatherings service: %w", err)
	}
	result, err := env.New(account).SearchItems(ctx, playfab.SearchFilter{})
	if err != nil {
		return nil, err
	}
	servers := discoveryServers(result.Items)
	firstCreator := 0
	for firstCreator < len(servers) && servers[firstCreator].Group == "featured" {
		firstCreator++
	}
	creators := servers[firstCreator:]
	rand.Shuffle(len(creators), func(i, j int) { creators[i], creators[j] = creators[j], creators[i] })
	return servers, nil
}

type serverProperties struct {
	ExperienceID string                     `json:"experienceId"`
	Host         string                     `json:"url"`
	Port         uint16                     `json:"port"`
	Rank         *int                       `json:"rank"`
	NewsTitle    string                     `json:"newsTitle"`
	News         string                     `json:"news"`
	Games        []gatherings.AvailableGame `json:"availableGames"`
}

// Rank presence determines the group; zero is a valid rank. Ranked entries precede creators.
func discoveryServers(items []playfab.Item) []FeaturedServer {
	type ranked struct {
		server FeaturedServer
		rank   *int
	}
	entries := make([]ranked, 0, len(items))
	seen := make(map[string]bool, len(items))
	for _, item := range items {
		if item.Hidden || item.ContentType != "3PP_V2.0" {
			continue
		}
		var props serverProperties
		if json.Unmarshal(item.DisplayProperties, &props) != nil {
			continue
		}
		address := ""
		if host := strings.TrimSpace(props.Host); host != "" && props.Port != 0 {
			address = net.JoinHostPort(host, strconv.Itoa(int(props.Port)))
		} else if id, err := uuid.Parse(props.ExperienceID); err == nil && id != uuid.Nil {
			address = GatheringTargetPrefix + id.String()
		}
		if address == "" || seen[address] {
			continue
		}
		group := "creator"
		if props.Rank != nil {
			group = "featured"
		}
		server := FeaturedServer{
			Name: localizedServerText(item.Title), Group: group, Address: address,
			Description: localizedServerText(item.Description),
			NewsTitle:   strings.TrimSpace(props.NewsTitle), News: strings.TrimSpace(props.News),
			Logo: serverImage(item.Images, "Icon", "Thumbnail"), Background: serverImage(item.Images, "Banner"),
			Screenshots: []Image{}, Games: []Game{},
		}
		if server.Name == "" {
			continue
		}
		seen[address] = true
		server.thumbnailURL = server.Logo.URL
		for _, image := range item.Images {
			if strings.EqualFold(image.Tag, "screenshot") && validArtworkURL(image.URL) {
				server.Screenshots = append(server.Screenshots, Image{URL: image.URL})
			}
		}
		for _, game := range props.Games {
			if strings.TrimSpace(game.Title) != "" || strings.TrimSpace(game.Subtitle) != "" {
				server.Games = append(server.Games, Game{
					Title: strings.TrimSpace(game.Title), Subtitle: strings.TrimSpace(game.Subtitle),
					Description: strings.TrimSpace(game.Description), Image: serverImage(item.Images, game.ImageTag),
				})
			}
		}
		entries = append(entries, ranked{server: server, rank: props.Rank})
	}
	slices.SortStableFunc(entries, func(a, b ranked) int {
		if a.rank == nil && b.rank == nil {
			return 0
		}
		if a.rank == nil {
			return 1
		}
		if b.rank == nil {
			return -1
		}
		if *a.rank > *b.rank {
			return -1
		}
		if *a.rank < *b.rank {
			return 1
		}
		return 0
	})
	servers := make([]FeaturedServer, len(entries))
	for i, entry := range entries {
		servers[i] = entry.server
	}
	return servers
}

func localizedServerText(text playfab.Dictionary[string]) string {
	for _, locale := range []string{"en-US", "en_US", "NEUTRAL", "neutral"} {
		if value := strings.TrimSpace(text[locale]); value != "" {
			return value
		}
	}
	return ""
}

// Artwork tags select their role independently of the service's image type.
func serverImage(images []playfab.Image, tags ...string) Image {
	for _, tag := range tags {
		for _, image := range images {
			if tag != "" && strings.EqualFold(image.Tag, tag) && validArtworkURL(image.URL) {
				return Image{URL: image.URL}
			}
		}
	}
	return Image{}
}
