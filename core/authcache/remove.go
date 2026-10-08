package authcache

import (
	"context"
	"errors"
	"fmt"
	"os"

	"github.com/hashimthearab/rust-mcbe/core/internal/lockfile"
)

const cacheLockSuffix = ".lock"

// Remove deletes both credential caches after any active refresh has finished.
// Leases follow the derived-to-OAuth order used by account operations; their
// stable lock files remain in place. The caller supplies its file remover.
func Remove(ctx context.Context, oauthPath string, remove func(string) error) (err error) {
	if oauthPath == "" {
		return nil
	}
	derivedPath := DerivedCachePath(oauthPath)
	derived, err := lockfile.AcquireContext(ctx, derivedPath+cacheLockSuffix)
	if err != nil {
		return fmt.Errorf("lock derived auth cache for removal: %w", err)
	}
	defer func() { err = errors.Join(err, derived.Close()) }()
	oauth, err := lockfile.AcquireContext(ctx, oauthPath+cacheLockSuffix)
	if err != nil {
		return fmt.Errorf("lock Microsoft auth cache for removal: %w", err)
	}
	defer func() { err = errors.Join(err, oauth.Close()) }()
	for _, path := range []string{oauthPath, derivedPath} {
		if removeErr := remove(path); removeErr != nil && !errors.Is(removeErr, os.ErrNotExist) {
			err = errors.Join(err, removeErr)
		}
	}
	return err
}
