package catalog

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"os"
	"path/filepath"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"github.com/sandertv/gophertunnel/minecraft/service/persona"
)

// ProfileAvatar fetches the full rendered character from the discovered persona
// service with the core's Minecraft service token, and caches it for Rust to load.
func ProfileAvatar(ctx context.Context, account *authcache.Account, xuid, artworkDir string) (Image, error) {
	if account == nil {
		return Image{}, errNoAccount
	}
	if artworkDir == "" {
		return Image{}, fmt.Errorf("profile avatar artwork cache unavailable")
	}
	discovery, err := service.Default(ctx)
	if err != nil {
		return Image{}, fmt.Errorf("discover persona service: %w", err)
	}
	env := new(persona.Environment)
	if err := discovery.Environment(env); err != nil {
		return Image{}, fmt.Errorf("resolve persona service: %w", err)
	}
	return cacheProfileAvatar(ctx, env, account, xuid, artworkDir)
}

// cacheProfileAvatar reads the source-backed avatar subtype and publishes a
// content-addressed image atomically so an account switch cannot reuse old art.
func cacheProfileAvatar(ctx context.Context, env *persona.Environment, tokens service.TokenSource, xuid, artworkDir string) (Image, error) {
	avatar, err := env.New(tokens).ProfileImage(ctx, xuid, persona.ImageAvatar)
	if err != nil {
		return Image{}, err
	}
	if err := os.MkdirAll(artworkDir, 0o700); err != nil {
		return Image{}, err
	}
	digest := sha256.Sum256(avatar.Data)
	path := filepath.Join(artworkDir, "persona-avatar-"+hex.EncodeToString(digest[:])+".img")
	if _, err := os.Stat(path); err == nil {
		return Image{Path: path}, nil
	}
	temporary, err := os.CreateTemp(artworkDir, ".persona-avatar-*")
	if err != nil {
		return Image{}, err
	}
	defer os.Remove(temporary.Name())
	_, err = temporary.Write(avatar.Data)
	if closeErr := temporary.Close(); err == nil {
		err = closeErr
	}
	if err != nil {
		return Image{}, err
	}
	if err := os.Rename(temporary.Name(), path); err != nil {
		return Image{}, err
	}
	return Image{Path: path}, nil
}
