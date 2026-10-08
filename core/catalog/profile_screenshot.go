package catalog

import (
	"context"
	"fmt"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"github.com/sandertv/gophertunnel/minecraft/service/gallery"
	"github.com/sandertv/gophertunnel/minecraft/service/persona"
)

// ProfileFeaturedScreenshot reads the showcased image marked featured from the
// discovered persona service. Its list length is never used as the local gallery count.
func ProfileFeaturedScreenshot(ctx context.Context, account *authcache.Account, xuid string) (Image, error) {
	if account == nil {
		return Image{}, errNoAccount
	}
	discovery, err := service.Default(ctx)
	if err != nil {
		return Image{}, fmt.Errorf("discover screenshot service: %w", err)
	}
	env := new(persona.Environment)
	if err := discovery.Environment(env); err != nil {
		return Image{}, fmt.Errorf("resolve persona service: %w", err)
	}
	return profileFeaturedScreenshot(ctx, &gallery.Environment{ServiceURI: env.ServiceURI}, account, xuid)
}

// profileFeaturedScreenshot selects only a featured HTTPS image; an empty showcase
// successfully selects vanilla's deterministic banner fallback.
func profileFeaturedScreenshot(ctx context.Context, env *gallery.Environment, tokens service.TokenSource, xuid string) (Image, error) {
	client, err := env.New(tokens)
	if err != nil {
		return Image{}, err
	}
	images, err := client.Images(ctx, xuid)
	if err != nil {
		return Image{}, err
	}
	for _, image := range images {
		if image.Featured && validArtworkURL(image.URL) {
			return Image{URL: image.URL}, nil
		}
	}
	return Image{}, nil
}
