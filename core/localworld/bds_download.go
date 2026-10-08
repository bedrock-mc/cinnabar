package localworld

import (
	"archive/zip"
	"cmp"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"hash"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"regexp"
	"slices"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/klauspost/compress/flate"
)

const (
	maxZipBytes         = 1 << 30
	unpackWorkers       = 8
	defaultStallTimeout = 30 * time.Second
	partSuffix          = ".zip.part"
)

// unpackLimits bound an archive by its listing before anything is written.
type unpackLimits struct {
	maxEntries, maxFileBytes, maxTotalBytes uint64
	minRatioSample                          uint64 // entries (and archives) with fewer compressed bytes skip the ratio checks
	maxEntryRatio, maxArchiveRatio          float64
}

// bdsLimits leave headroom over the Linux 1.26.32.2 build: 9,761 entries, 317 MB expanded, largest file
// 233 MB, worst entry ratio about 19.7, aggregate 3.6.
var bdsLimits = unpackLimits{
	maxEntries: 65_536, maxFileBytes: 1 << 30, maxTotalBytes: 4 << 30,
	minRatioSample: 4096, maxEntryRatio: 500, maxArchiveRatio: 100,
}

var (
	errDownloadStalled = errors.New("no data received for too long")
	installDirName     = regexp.MustCompile(`^\d+(?:\.\d+)+(?:\.partial)?$`)
)

type progressWriter struct {
	p     *Provisioner
	ver   string
	total int64
	done  int64
	hash  io.Writer
	out   io.Writer
}

func (w *progressWriter) Write(b []byte) (int, error) {
	n, err := w.out.Write(b)
	_, _ = w.hash.Write(b[:n])
	w.done += int64(n)
	w.p.setOp(SetupDownloading, w.ver, w.done, w.total)
	if w.done > maxZipBytes {
		return n, errZipTooLarge
	}
	return n, err
}

var errZipTooLarge = errors.New("localworld: dedicated server download exceeds size limit")

// idleReader cancels its request once no bytes arrive for timeout.
type idleReader struct {
	r       io.Reader
	timer   *time.Timer
	timeout time.Duration
}

func (i *idleReader) Read(b []byte) (int, error) {
	n, err := i.r.Read(b)
	if n > 0 {
		i.timer.Reset(i.timeout)
	}
	return n, err
}

func (p *Provisioner) downloadsDir() string { return filepath.Join(p.Root, "downloads") }

// download fetches version's archive into a .zip.part that a failed attempt leaves behind for a Range resume.
func (p *Provisioner) download(ctx context.Context, version, link string) (path, sum string, size int64, err error) {
	p.setOp(SetupDownloading, version, 0, 0)
	dir := p.downloadsDir()
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return "", "", 0, err
	}
	path = filepath.Join(dir, "bedrock-server-"+version+partSuffix)
	removeStaleParts(dir, filepath.Base(path))
	file, err := os.OpenFile(path, os.O_CREATE|os.O_RDWR, 0o600)
	if err != nil {
		return "", "", 0, err
	}
	digest := sha256.New()
	size, err = p.fetchInto(ctx, file, digest, version, link)
	if closeErr := file.Close(); err == nil {
		err = closeErr
	}
	if errors.Is(err, errZipTooLarge) {
		os.Remove(path)
	}
	if err != nil {
		return "", "", 0, fmt.Errorf("localworld: download dedicated server: %w", err)
	}
	return path, hex.EncodeToString(digest.Sum(nil)), size, nil
}

// fetchInto appends the rest of link to file (hashing what is already there), restarting when the server cannot resume.
func (p *Provisioner) fetchInto(ctx context.Context, file *os.File, digest hash.Hash, version, link string) (int64, error) {
	offset, err := io.Copy(digest, file)
	if err != nil {
		return 0, err
	}
	stall := p.stallTimeout
	if stall <= 0 {
		stall = defaultStallTimeout
	}
	ctx, cancel := context.WithCancelCause(ctx)
	defer cancel(nil)
	timer := time.AfterFunc(stall, func() { cancel(errDownloadStalled) })
	defer timer.Stop()
	for attempt := 0; ; attempt++ {
		if offset > maxZipBytes {
			offset = 0
		}
		if offset == 0 {
			if err := restart(file, digest); err != nil {
				return 0, err
			}
		}
		timer.Reset(stall)
		resp, err := p.get(ctx, link, offset)
		if err != nil {
			return 0, stalled(ctx, err)
		}
		total, resumable := resumeTotal(resp, offset)
		if !resumable {
			resp.Body.Close()
			if attempt > 0 {
				return 0, errors.New("dedicated server download cannot resume")
			}
			offset = 0
			continue
		}
		if resp.StatusCode == http.StatusOK {
			offset = 0
			if err := restart(file, digest); err != nil {
				resp.Body.Close()
				return 0, err
			}
		}
		w := &progressWriter{p: p, ver: version, total: total, done: offset, hash: digest, out: file}
		p.setOp(SetupDownloading, version, offset, total)
		_, copyErr := io.Copy(w, &idleReader{r: resp.Body, timer: timer, timeout: stall})
		resp.Body.Close()
		if copyErr != nil {
			return 0, stalled(ctx, copyErr)
		}
		if total > 0 && w.done != total {
			return 0, errors.New("dedicated server download was truncated")
		}
		return w.done, nil
	}
}

// resumeTotal is the archive's full size (-1 when unknown); resumable is false when the reply cannot continue offset.
func resumeTotal(resp *http.Response, offset int64) (total int64, resumable bool) {
	switch resp.StatusCode {
	case http.StatusOK:
		if resp.ContentLength >= 0 {
			return resp.ContentLength, true
		}
		return -1, true
	case http.StatusPartialContent:
		start, total, ok := parseContentRange(resp.Header.Get("Content-Range"))
		return total, ok && start == offset
	}
	return -1, false
}

// parseContentRange reads `bytes start-end/total`; total is -1 for `*`.
func parseContentRange(header string) (start, total int64, ok bool) {
	spec, found := strings.CutPrefix(header, "bytes ")
	span, size, found2 := strings.Cut(spec, "/")
	first, _, found3 := strings.Cut(span, "-")
	if !found || !found2 || !found3 {
		return 0, 0, false
	}
	start, err := strconv.ParseInt(first, 10, 64)
	if err != nil {
		return 0, 0, false
	}
	if size == "*" {
		return start, -1, true
	}
	total, err = strconv.ParseInt(size, 10, 64)
	return start, total, err == nil && start < total
}

func restart(file *os.File, digest hash.Hash) error {
	digest.Reset()
	if err := file.Truncate(0); err != nil {
		return err
	}
	_, err := file.Seek(0, io.SeekStart)
	return err
}

func stalled(ctx context.Context, err error) error {
	if errors.Is(context.Cause(ctx), errDownloadStalled) {
		return errDownloadStalled
	}
	return err
}

// removeStaleParts deletes interrupted downloads other than keep.
func removeStaleParts(dir, keep string) {
	entries, _ := os.ReadDir(dir)
	for _, entry := range entries {
		if name := entry.Name(); name != keep && strings.HasSuffix(name, partSuffix) && entry.Type().IsRegular() {
			_ = os.Remove(filepath.Join(dir, name))
		}
	}
}

// pruneOldInstalls removes other versions' install folders and every leftover download.
func (p *Provisioner) pruneOldInstalls(keep string) {
	removeStaleParts(p.downloadsDir(), "")
	entries, _ := os.ReadDir(p.Root)
	for _, entry := range entries {
		if !entry.IsDir() || entry.Name() == keep || !installDirName.MatchString(entry.Name()) {
			continue
		}
		dir := filepath.Join(p.Root, entry.Name())
		if !releaseWorldLinks(dir) {
			p.log().Warn("kept an old dedicated server install whose worlds folder is not empty", "version", entry.Name())
			continue
		}
		if err := os.RemoveAll(dir); err != nil {
			p.log().Warn("remove old dedicated server install", "version", entry.Name(), "error", err)
		}
	}
}

// releaseWorldLinks drops world links and empty mount points from an install; false when anything else remains,
// so deleting the install can never reach world data.
func releaseWorldLinks(install string) bool {
	worlds := filepath.Join(install, "worlds")
	entries, err := os.ReadDir(worlds)
	if errors.Is(err, os.ErrNotExist) {
		return true
	}
	if err != nil {
		return false
	}
	for _, entry := range entries {
		unlinkWorld(filepath.Join(worlds, entry.Name()))
	}
	left, err := os.ReadDir(worlds)
	return err == nil && len(left) == 0
}

func (p *Provisioner) unpack(zipPath, version, link, sum string, size int64) (string, error) {
	reader, err := zip.OpenReader(zipPath)
	if err != nil {
		return "", fmt.Errorf("localworld: dedicated server archive is corrupt: %w", err)
	}
	defer reader.Close()
	reader.RegisterDecompressor(zip.Deflate, func(in io.Reader) io.ReadCloser { return flate.NewReader(in) })
	final := filepath.Join(p.Root, version)
	partial := final + ".partial"
	if err := os.RemoveAll(partial); err != nil {
		return "", err
	}
	if err := os.MkdirAll(partial, 0o700); err != nil {
		return "", err
	}
	if err := extractArchive(partial, reader.File, bdsLimits); err != nil {
		os.RemoveAll(partial)
		return "", err
	}
	if _, err := os.Stat(filepath.Join(partial, p.binaryName())); err != nil {
		os.RemoveAll(partial)
		return "", errors.New("localworld: dedicated server archive has no server binary")
	}
	_ = os.Chmod(filepath.Join(partial, p.binaryName()), 0o755)
	goos, arch := p.platform()
	raw, _ := json.MarshalIndent(manifest{
		Version: version, URL: link, ZipSHA256: sum, ZipBytes: size, Platform: goos + "/" + arch,
		DownloadedAt: time.Now().Unix(), ClientVersion: p.prefix(),
	}, "", "  ")
	if err := os.WriteFile(filepath.Join(partial, "manifest.json"), raw, 0o600); err != nil {
		os.RemoveAll(partial)
		return "", err
	}
	if err := os.RemoveAll(final); err != nil {
		return "", err
	}
	if err := os.Rename(partial, final); err != nil {
		return "", err
	}
	return filepath.Join(final, p.binaryName()), nil
}

type plannedFile struct {
	entry  *zip.File
	target string
}

// extractArchive checks the whole listing against limits, creates every folder once, then writes files in parallel.
func extractArchive(root string, entries []*zip.File, limits unpackLimits) error {
	files, dirs, err := planExtraction(root, entries, limits)
	if err != nil {
		return err
	}
	for _, dir := range dirs {
		if err := os.MkdirAll(dir, 0o700); err != nil {
			return err
		}
	}
	jobs := make(chan plannedFile)
	var (
		wg       sync.WaitGroup
		once     sync.Once
		firstErr error
		failed   = make(chan struct{})
	)
	for range min(unpackWorkers, len(files)) {
		wg.Go(func() {
			buf := make([]byte, 256<<10)
			for job := range jobs {
				if err := extractFile(job, buf); err != nil {
					once.Do(func() { firstErr = err; close(failed) })
				}
			}
		})
	}
feed:
	for _, file := range files {
		select {
		case jobs <- file:
		case <-failed:
			break feed
		}
	}
	close(jobs)
	wg.Wait()
	return firstErr
}

// planExtraction validates every entry and returns the files (largest first) and the folders they need.
// Symlinks and other special entries are skipped.
func planExtraction(root string, entries []*zip.File, limits unpackLimits) ([]plannedFile, []string, error) {
	if uint64(len(entries)) > limits.maxEntries {
		return nil, nil, fmt.Errorf("localworld: dedicated server archive has more than %d entries", limits.maxEntries)
	}
	var (
		files             []plannedFile
		dirSet            = map[string]struct{}{}
		seen              = map[string]struct{}{}
		total, compressed uint64
	)
	for _, entry := range entries {
		target, err := entryTarget(root, entry.Name)
		if err != nil {
			return nil, nil, err
		}
		mode := entry.Mode()
		if mode.IsDir() {
			dirSet[target] = struct{}{}
			continue
		}
		if !mode.IsRegular() {
			continue
		}
		// Case-folded, so parallel writers never share a file on case-insensitive disks.
		key := strings.ToLower(target)
		if _, dup := seen[key]; dup {
			return nil, nil, fmt.Errorf("localworld: archive entry %q is duplicated", entry.Name)
		}
		seen[key] = struct{}{}
		size, packed := entry.UncompressedSize64, entry.CompressedSize64
		if size > limits.maxFileBytes {
			return nil, nil, errors.New("localworld: dedicated server archive exceeds size limit")
		}
		if packed >= limits.minRatioSample && float64(size) > limits.maxEntryRatio*float64(packed) {
			return nil, nil, fmt.Errorf("localworld: archive entry %q is compressed implausibly well", entry.Name)
		}
		total += size
		compressed += packed
		if total > limits.maxTotalBytes {
			return nil, nil, errors.New("localworld: dedicated server archive exceeds size limit")
		}
		dirSet[filepath.Dir(target)] = struct{}{}
		files = append(files, plannedFile{entry: entry, target: target})
	}
	if compressed >= limits.minRatioSample && float64(total) > limits.maxArchiveRatio*float64(compressed) {
		return nil, nil, errors.New("localworld: dedicated server archive is compressed implausibly well")
	}
	// Largest first, so the server binary never trails the small files.
	slices.SortFunc(files, func(a, b plannedFile) int {
		return cmp.Compare(b.entry.UncompressedSize64, a.entry.UncompressedSize64)
	})
	dirs := make([]string, 0, len(dirSet))
	for dir := range dirSet {
		dirs = append(dirs, dir)
	}
	slices.Sort(dirs)
	return files, dirs, nil
}

// entryTarget is the path of an archive entry under root, rejecting names that escape it.
func entryTarget(root, name string) (string, error) {
	local := filepath.FromSlash(name)
	if filepath.IsAbs(local) || strings.HasPrefix(name, "/") {
		return "", fmt.Errorf("localworld: archive entry %q is absolute", name)
	}
	target := filepath.Join(root, local)
	if rel, err := filepath.Rel(root, target); err != nil || rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
		return "", fmt.Errorf("localworld: archive entry %q escapes the install directory", name)
	}
	return target, nil
}

// extractFile writes one planned entry, refusing more bytes than its listing declared.
func extractFile(job plannedFile, buf []byte) error {
	in, err := job.entry.Open()
	if err != nil {
		return err
	}
	defer in.Close()
	declared := int64(job.entry.UncompressedSize64)
	out, err := os.OpenFile(job.target, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0o600|(job.entry.Mode().Perm()&0o100))
	if err != nil {
		return err
	}
	n, copyErr := io.CopyBuffer(struct{ io.Writer }{out}, io.LimitReader(in, declared+1), buf)
	if err := errors.Join(copyErr, out.Close()); err != nil {
		return err
	}
	if n > declared {
		return errors.New("localworld: dedicated server archive exceeds size limit")
	}
	return nil
}
