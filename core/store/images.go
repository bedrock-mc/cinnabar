package store

import (
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/imagecache"
)

// ErrImageRejected identifies a refused URL or image payload.
var ErrImageRejected = imagecache.ErrRejected

// Image is a cached offer image on local disk.
type Image = imagecache.Image

// newImageCache supplies Marketplace's download and disk limits to the shared cache.
func newImageCache(dir string) *imagecache.Cache {
	return imagecache.New(dir, imagecache.Config{
		MaxBytes: 4 << 20, MaxFiles: 512, MaxDirBytes: 256 << 20,
		Timeout: 20 * time.Second, MaxRedirects: 3, MaxURLBytes: 1024,
		UserAgent: "libhttpclient/1.0.0.0",
	})
}
