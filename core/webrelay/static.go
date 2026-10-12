package webrelay

import (
	"bytes"
	"errors"
	"io"
	"io/fs"
	"mime"
	"net/http"
	"os"
	"path"
	"path/filepath"
	"runtime/debug"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/klauspost/compress/gzip"
	"github.com/klauspost/compress/zstd"
)

// DecodedLengthHeader carries a compressed response's decoded size, which a page's download
// progress counts against since Content-Length is the compressed size.
const DecodedLengthHeader = "X-Decoded-Length"

// staticCodings are the content codings StaticFiles sends, in preference order: zstd where the
// browser accepts it, else gzip. Precompressed siblings carry the coding's file suffix.
var staticCodings = []staticCoding{
	{name: "zstd", suffix: ".zst", newWriter: newZstdWriter},
	{name: "gzip", suffix: ".gz", newWriter: newGzipWriter},
}

type staticCoding struct {
	name      string
	suffix    string
	newWriter func(io.Writer) (io.WriteCloser, error)
}

// minCompressedSaving is the smallest share of a file compression must save to be sent;
// already-compressed files go out as stored.
const minCompressedSaving = 0.05

// StaticFiles serves dir as http.FileServer does, but sends whole files compressed as the
// browser accepts and marks every file for revalidation, so a repeat visit re-downloads only
// files that changed. A precompressed sibling (name.zst, name.gz) at least as new as its file is
// sent as is; other files compress on first request and stay cached while unchanged.
// Byte-range requests, which the sound bank relies on, get the stored bytes, and a file only
// ever read by range is never compressed. A file that compresses by less than
// minCompressedSaving is sent stored.
func StaticFiles(dir string) *StaticFileServer {
	return &StaticFileServer{
		dir:     dir,
		raw:     http.FileServer(http.Dir(dir)),
		entries: make(map[string]*staticEntry),
	}
}

// Warm compresses every regular file below the directory of at most limit bytes (0: any size)
// with the preferred coding, so first requests find it ready. A limit leaves out files read
// only by range, such as the sound bank, whose compressed copy no request would use.
func (files *StaticFileServer) Warm(limit int64) {
	_ = filepath.WalkDir(files.dir, func(full string, entry fs.DirEntry, err error) error {
		if err != nil || !entry.Type().IsRegular() || hasCodingSuffix(full) {
			return nil
		}
		if info, err := entry.Info(); err == nil && (limit == 0 || info.Size() <= limit) {
			files.encoded(full, info, staticCodings[0])
		}
		return nil
	})
	// Reading and encoding every file leaves garbage worth many times the cache.
	debug.FreeOSMemory()
}

// hasCodingSuffix reports whether full is itself a precompressed sibling.
func hasCodingSuffix(full string) bool {
	for _, coding := range staticCodings {
		if strings.HasSuffix(full, coding.suffix) {
			return true
		}
	}
	return false
}

// StaticFileServer is the handler StaticFiles returns.
type StaticFileServer struct {
	dir     string
	raw     http.Handler
	mu      sync.Mutex
	entries map[string]*staticEntry
}

// staticEntry is one file's version and its compressed bodies, encoded at most once each.
type staticEntry struct {
	size    int64
	modTime time.Time
	mu      sync.Mutex
	bodies  map[string][]byte // nil body: compression did not pay
}

func (files *StaticFileServer) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	name := path.Clean("/" + r.URL.Path)
	full := filepath.Join(files.dir, filepath.FromSlash(name))
	header := w.Header()
	// A page and its files always revalidate, so a rebuilt site never runs stale.
	header.Set("Cache-Control", "no-cache")
	info, err := os.Stat(full)
	if err != nil || !info.Mode().IsRegular() || (r.Method != http.MethodGet && r.Method != http.MethodHead) {
		files.raw.ServeHTTP(w, r)
		return
	}
	version := strconv.FormatInt(info.ModTime().UnixNano(), 36) + "-" + strconv.FormatInt(info.Size(), 36)
	header.Add("Vary", "Accept-Encoding")
	coding, ok := acceptedCoding(r.Header.Get("Accept-Encoding"))
	if !ok || r.Header.Get("Range") != "" {
		header.Set("ETag", `"`+version+`"`)
		files.raw.ServeHTTP(w, r)
		return
	}
	body := files.encoded(full, info, coding)
	if body == nil {
		header.Set("ETag", `"`+version+`"`)
		files.raw.ServeHTTP(w, r)
		return
	}
	contentType := mime.TypeByExtension(path.Ext(name))
	if contentType == "" {
		contentType = "application/octet-stream"
	}
	header.Set("Content-Type", contentType)
	header.Set("Content-Encoding", coding.name)
	header.Set("ETag", `"`+version+"-"+coding.name+`"`)
	header.Set(DecodedLengthHeader, strconv.FormatInt(info.Size(), 10))
	// ServeContent leaves Content-Length out of encoded responses; this one is always whole.
	header.Set("Content-Length", strconv.Itoa(len(body)))
	http.ServeContent(w, r, name, info.ModTime(), bytes.NewReader(body))
}

// encoded returns the file at full compressed with coding, or nil when it does not pay: a
// precompressed sibling at least as new as the file, else this version's own encoding, each
// produced once and cached while the file is unchanged.
func (files *StaticFileServer) encoded(full string, info os.FileInfo, coding staticCoding) []byte {
	files.mu.Lock()
	entry := files.entries[full]
	if entry == nil || entry.size != info.Size() || !entry.modTime.Equal(info.ModTime()) {
		entry = &staticEntry{size: info.Size(), modTime: info.ModTime(), bodies: make(map[string][]byte)}
		files.entries[full] = entry
	}
	files.mu.Unlock()
	entry.mu.Lock()
	defer entry.mu.Unlock()
	if body, done := entry.bodies[coding.name]; done {
		return body
	}
	if sibling, err := os.Stat(full + coding.suffix); err == nil && !sibling.ModTime().Before(info.ModTime()) {
		if body, err := os.ReadFile(full + coding.suffix); err == nil {
			entry.bodies[coding.name] = body
			return body
		}
	}
	body, err := compressFile(full, info.Size(), coding)
	if err != nil {
		return nil // a file changing under the read is retried by the next request
	}
	entry.bodies[coding.name] = body
	return body
}

// compressFile streams the file at full, which must still hold size bytes, through coding;
// nil when compression saves less than minCompressedSaving. Only the output is held in memory,
// and only whole requests reach it, so a file read only by range is never compressed.
func compressFile(full string, size int64, coding staticCoding) ([]byte, error) {
	file, err := os.Open(full)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	// One byte past size shows the file grew while it was read.
	reader := &countingReader{reader: io.LimitReader(file, size+1)}
	compressed, err := compress(reader, coding, int(size/4))
	if err != nil {
		return nil, err
	}
	if reader.count != size {
		return nil, io.ErrUnexpectedEOF
	}
	if !pays(len(compressed), int(size)) {
		return nil, nil
	}
	// The buffer grew past the output; the cache keeps only the bytes.
	return bytes.Clone(compressed), nil
}

// compress reads source to its end through coding, growing from estimate bytes.
func compress(source io.Reader, coding staticCoding, estimate int) ([]byte, error) {
	var output bytes.Buffer
	output.Grow(estimate)
	writer, err := coding.newWriter(&output)
	if err != nil {
		return nil, err
	}
	_, copyErr := io.Copy(writer, source)
	if err := writer.Close(); copyErr != nil || err != nil {
		return nil, errors.Join(copyErr, err)
	}
	return output.Bytes(), nil
}

// countingReader counts the bytes read through it.
type countingReader struct {
	reader io.Reader
	count  int64
}

func (counter *countingReader) Read(buffer []byte) (int, error) {
	n, err := counter.reader.Read(buffer)
	counter.count += int64(n)
	return n, err
}

// pays reports whether compressing raw bytes to compressed bytes saves minCompressedSaving.
func pays(compressed, raw int) bool {
	return compressed > 0 && float64(compressed) <= float64(raw)*(1-minCompressedSaving)
}

// acceptedCoding picks the first of staticCodings the Accept-Encoding header allows.
func acceptedCoding(accept string) (staticCoding, bool) {
	allowed := make(map[string]bool)
	for _, part := range strings.Split(accept, ",") {
		token, params, _ := strings.Cut(strings.TrimSpace(part), ";")
		token = strings.ToLower(strings.TrimSpace(token))
		if q, found := strings.CutPrefix(strings.TrimSpace(params), "q="); found {
			if value, err := strconv.ParseFloat(q, 64); err == nil && value == 0 {
				continue
			}
		}
		allowed[token] = true
	}
	for _, coding := range staticCodings {
		if allowed[coding.name] {
			return coding, true
		}
	}
	return staticCoding{}, false
}

// zstdWindow is the largest window HTTP's zstd coding lets a sender use (RFC 9659).
const zstdWindow = 8 << 20

// sharedZstd is the one zstd encoder every compression reuses in turn, since an encoder's
// match tables are large; a writer holds it from newZstdWriter until Close.
var sharedZstd struct {
	sync.Mutex
	encoder *zstd.Encoder
}

type sharedZstdWriter struct{ *zstd.Encoder }

func (writer sharedZstdWriter) Close() error {
	defer sharedZstd.Unlock()
	return writer.Encoder.Close()
}

func newZstdWriter(output io.Writer) (io.WriteCloser, error) {
	sharedZstd.Lock()
	if sharedZstd.encoder == nil {
		encoder, err := zstd.NewWriter(output,
			zstd.WithEncoderLevel(zstd.SpeedBetterCompression),
			zstd.WithWindowSize(zstdWindow),
			zstd.WithEncoderConcurrency(1))
		if err != nil {
			sharedZstd.Unlock()
			return nil, err
		}
		sharedZstd.encoder = encoder
	} else {
		sharedZstd.encoder.Reset(output)
	}
	return sharedZstdWriter{sharedZstd.encoder}, nil
}

func newGzipWriter(output io.Writer) (io.WriteCloser, error) {
	return gzip.NewWriterLevel(output, gzip.BestCompression)
}
