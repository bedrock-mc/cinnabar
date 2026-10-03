package main

import (
	"fmt"
	"log/slog"
	"os"
	"path/filepath"
	"time"

	"github.com/df-mc/dragonfly/server/world"
	"github.com/google/uuid"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/experience"
	"github.com/hashimthearab/rust-mcbe/tools/localserver/extension"
)

// startClientParts sets up the server half of client parts for the -extension flags: it reads
// the server key and the bundles, takes the next revision, signs the offer and writes it as an
// optional resource pack, so it must run before server.UserConfig.Config loads the packs.
func startClientParts(cfg settings, log *slog.Logger) (*extension.Server, error) {
	seed, err := os.ReadFile(cfg.extensionKey)
	if err != nil {
		return nil, fmt.Errorf("read -extension-key: %w", err)
	}
	key, err := extension.ParseSeed(string(seed))
	if err != nil {
		return nil, fmt.Errorf("-extension-key %s: %w", cfg.extensionKey, err)
	}
	bundles, err := extension.ReadBundles(cfg.extensionCXB)
	if err != nil {
		return nil, fmt.Errorf("-extension-cxb: %w", err)
	}
	revision, err := extension.NextRevision(filepath.Join(cfg.dir, extension.RevisionFile))
	if err != nil {
		return nil, err
	}
	ext, err := extension.NewServer(extension.Config{
		Key:      key,
		Audience: cfg.extensionAudience,
		Revision: revision,
		Bundles:  bundles,
		Log:      log,
	})
	if err != nil {
		return nil, fmt.Errorf("-extension-audience or -extension-cxb: %w", err)
	}
	if err := ext.WriteMarkerPack(cfg.resourcesDir()); err != nil {
		return nil, fmt.Errorf("write the client part offer: %w", err)
	}
	offer := ext.Offer()
	ids := make([]string, len(offer.Packages))
	for i, p := range offer.Packages {
		ids[i] = p.ID
	}
	log.Info("client parts offered", "audience", offer.Audience, "revision", offer.Revision,
		"expires", time.Unix(int64(offer.ExpiresUnix), 0).UTC(), "packages", ids)
	return ext, nil
}

// deliverClientMessages passes each client part message to the Experience whose id is the bundle
// id, as that player's callback; a player who has left, or a server without that Experience,
// drops it.
func deliverClientMessages(ext *extension.Server, players func(uuid.UUID) (*world.EntityHandle, bool), host *experience.Host, log *slog.Logger) {
	ext.OnClientMessage(func(player uuid.UUID, exp, channel string, schema uint16, payload []experience.Scalar) {
		handle, ok := players(player)
		if !ok || host == nil || !host.DeliverClientMessage(handle, exp, channel, schema, payload) {
			log.Debug("client part message dropped", "experience", exp, "channel", channel, "schema", schema)
		}
	})
}
