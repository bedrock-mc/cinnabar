// Package imagecache downloads public HTTPS images into a bounded disk cache.
package imagecache

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"time"
)

// Config preserves the download and storage limits of each image surface.
type Config struct {
	MaxBytes     int64
	MaxFiles     int
	MaxDirBytes  int64 // zero leaves only the file-count bound
	Timeout      time.Duration
	MaxRedirects int
	MaxURLBytes  int // zero leaves the URL length unbounded
	UserAgent    string
	Extension    string            // empty selects an extension from the image signature
	Transport    http.RoundTripper // nil connects only to public addresses
}

// ErrRejected is returned for a URL or payload the image cache refuses.
var ErrRejected = errors.New("image cache: image rejected")

// Image is a cached image on local disk.
type Image struct {
	Path        string `json:"path"`
	ContentType string `json:"content_type"`
}

// Cache downloads images over HTTPS into a bounded directory; it refuses local and private
// addresses and anything that is not a PNG, JPEG, GIF or BMP.
type Cache struct {
	dir  string
	http *http.Client
	cfg  Config

	mu sync.Mutex
}

var imageExtensions = map[string]string{"image/png": ".png", "image/jpeg": ".jpg", "image/gif": ".gif", "image/bmp": ".bmp"}

// publicTransport shares bounded idle connections across catalog and store downloads.
var publicTransport = &http.Transport{
	DialContext:         publicDialer(net.DefaultResolver.LookupIPAddr, (&net.Dialer{}).DialContext),
	TLSHandshakeTimeout: 10 * time.Second,
	IdleConnTimeout:     90 * time.Second, MaxIdleConns: 100,
}

// New returns a cache rooted at dir, created on first use.
func New(dir string, cfg Config) *Cache {
	transport := cfg.Transport
	if transport == nil {
		transport = publicTransport
	}
	client := &http.Client{
		Transport: transport, Timeout: cfg.Timeout,
		CheckRedirect: func(req *http.Request, via []*http.Request) error {
			if len(via) > cfg.MaxRedirects || !ValidURL(req.URL.String()) {
				return ErrRejected
			}
			return nil
		},
	}
	return &Cache{dir: dir, http: client, cfg: cfg}
}

// ValidURL accepts HTTPS image URLs without embedded credentials.
func ValidURL(raw string) bool {
	u, err := url.Parse(raw)
	return err == nil && u.Scheme == "https" && u.Hostname() != "" && u.User == nil
}

// publicDialer validates every resolved address before trying each within a shared deadline.
func publicDialer(
	lookup func(context.Context, string) ([]net.IPAddr, error),
	dial func(context.Context, string, string) (net.Conn, error),
) func(ctx context.Context, network, addr string) (net.Conn, error) {
	return func(ctx context.Context, network, addr string) (net.Conn, error) {
		ctx, cancel := context.WithTimeout(ctx, 10*time.Second)
		defer cancel()
		host, port, err := net.SplitHostPort(addr)
		if err != nil {
			return nil, ErrRejected
		}
		ips, err := lookup(ctx, host)
		if ctx.Err() != nil {
			return nil, ctx.Err()
		}
		if err != nil || len(ips) == 0 {
			return nil, ErrRejected
		}
		for _, ip := range ips {
			if !publicIP(ip.IP) {
				return nil, ErrRejected
			}
		}
		deadline, _ := ctx.Deadline()
		for i, ip := range ips {
			if ctx.Err() != nil {
				return nil, ctx.Err()
			}
			// Reserve time for the remaining addresses if this one cannot connect.
			attempt, stop := context.WithTimeout(ctx, time.Until(deadline)/time.Duration(len(ips)-i))
			conn, dialErr := dial(attempt, network, net.JoinHostPort(ip.IP.String(), port))
			stop()
			if dialErr == nil {
				return conn, nil
			}
			err = dialErr
		}
		if ctx.Err() != nil {
			return nil, ctx.Err()
		}
		return nil, err
	}
}

// publicIP reports whether an address can be reached outside local networks.
func publicIP(ip net.IP) bool {
	return ip.IsGlobalUnicast() && !ip.IsPrivate() && !ip.IsLoopback() && !ip.IsLinkLocalUnicast() && !ip.IsUnspecified()
}

// Fetch returns the cached image for rawURL, downloading it when absent.
func (c *Cache) Fetch(ctx context.Context, rawURL string) (Image, error) {
	u, err := url.Parse(rawURL)
	if err != nil || !ValidURL(rawURL) || (c.cfg.MaxURLBytes > 0 && len(rawURL) > c.cfg.MaxURLBytes) {
		return Image{}, ErrRejected
	}
	sum := sha256.Sum256([]byte(u.String()))
	stem := filepath.Join(c.dir, hex.EncodeToString(sum[:]))
	for _, ext := range imageExtensions {
		if c.cfg.Extension != "" {
			ext = c.cfg.Extension
		}
		path := stem + ext
		if image, ok := c.cached(path); ok {
			return image, nil
		}
		if c.cfg.Extension != "" {
			break
		}
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, u.String(), nil)
	if err != nil {
		return Image{}, ErrRejected
	}
	req.Header.Set("User-Agent", c.cfg.UserAgent)
	resp, err := c.http.Do(req)
	if err != nil {
		return Image{}, fmt.Errorf("image cache: fetch image: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return Image{}, ErrRejected
	}
	data, err := io.ReadAll(io.LimitReader(resp.Body, c.cfg.MaxBytes+1))
	if err != nil {
		return Image{}, fmt.Errorf("image cache: read image: %w", err)
	}
	if int64(len(data)) > c.cfg.MaxBytes {
		return Image{}, ErrRejected
	}
	contentType := sniffImage(data)
	ext, ok := imageExtensions[contentType]
	if !ok {
		return Image{}, ErrRejected
	}
	if c.cfg.Extension != "" {
		ext = c.cfg.Extension
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if err := os.MkdirAll(c.dir, 0o700); err != nil {
		return Image{}, fmt.Errorf("image cache: %w", err)
	}
	temp, err := os.CreateTemp(c.dir, ".image-*")
	if err != nil {
		return Image{}, fmt.Errorf("image cache: %w", err)
	}
	name := temp.Name()
	_, writeErr := temp.Write(data)
	closeErr := temp.Close()
	if err := errors.Join(writeErr, closeErr); err != nil {
		_ = os.Remove(name)
		return Image{}, fmt.Errorf("image cache: %w", err)
	}
	if err := os.Chmod(name, 0o600); err != nil {
		_ = os.Remove(name)
		return Image{}, fmt.Errorf("image cache: %w", err)
	}
	if err := os.Rename(name, stem+ext); err != nil {
		_ = os.Remove(name)
		return Image{}, fmt.Errorf("image cache: %w", err)
	}
	c.evictLocked()
	return Image{Path: stem + ext, ContentType: contentType}, nil
}

// sniffImage identifies supported image formats by their signatures.
func sniffImage(data []byte) string {
	switch {
	case len(data) >= 8 && string(data[:8]) == "\x89PNG\r\n\x1a\n":
		return "image/png"
	case len(data) >= 3 && data[0] == 0xFF && data[1] == 0xD8 && data[2] == 0xFF:
		return "image/jpeg"
	case len(data) >= 6 && (string(data[:6]) == "GIF87a" || string(data[:6]) == "GIF89a"):
		return "image/gif"
	case len(data) >= 2 && data[0] == 'B' && data[1] == 'M':
		return "image/bmp"
	}
	return ""
}

// cached checks the file size and signature before returning a previous download.
func (c *Cache) cached(path string) (Image, bool) {
	file, err := os.Open(path)
	if err != nil {
		return Image{}, false
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil || !info.Mode().IsRegular() || info.Size() > c.cfg.MaxBytes {
		return Image{}, false
	}
	var header [8]byte
	n, _ := io.ReadFull(file, header[:])
	contentType := sniffImage(header[:n])
	if contentType == "" {
		return Image{}, false
	}
	now := time.Now()
	_ = os.Chtimes(path, now, now)
	return Image{Path: path, ContentType: contentType}, true
}

// Prune removes old files until the configured directory bounds are met.
func (c *Cache) Prune() {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.evictLocked()
}

// evictLocked removes the least recently used files beyond the count and size bounds.
func (c *Cache) evictLocked() {
	entries, err := os.ReadDir(c.dir)
	if err != nil {
		return
	}
	type file struct {
		path string
		size int64
		mod  time.Time
	}
	var files []file
	var total int64
	for _, entry := range entries {
		info, err := entry.Info()
		if err != nil || !info.Mode().IsRegular() || strings.HasPrefix(entry.Name(), ".") {
			continue
		}
		files = append(files, file{filepath.Join(c.dir, entry.Name()), info.Size(), info.ModTime()})
		total += info.Size()
	}
	sort.Slice(files, func(i, j int) bool { return files[i].mod.Before(files[j].mod) })
	for len(files) > c.cfg.MaxFiles || (c.cfg.MaxDirBytes > 0 && total > c.cfg.MaxDirBytes) {
		if len(files) == 0 {
			return
		}
		_ = os.Remove(files[0].path)
		total -= files[0].size
		files = files[1:]
	}
}
