package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"runtime"
	"sort"
	"strings"
	"time"
)

var sourceIDPattern = regexp.MustCompile(`^[a-z0-9][a-z0-9-]{0,63}$`)
var commitPattern = regexp.MustCompile(`^[0-9a-f]{40}$`)

type manifest struct {
	Schema         int      `json:"schema"`
	Protocol       protocol `json:"protocol"`
	ArtifactPolicy string   `json:"artifact_policy"`
	Limits         limits   `json:"limits"`
	Sources        []source `json:"sources"`
}

type protocol struct {
	GameVersion     string `json:"game_version"`
	ProtocolVersion int    `json:"protocol_version"`
}

type limits struct {
	MaxSources           int   `json:"max_sources"`
	MaxFilesPerSource    int   `json:"max_files_per_source"`
	MaxFileBytes         int64 `json:"max_file_bytes"`
	MaxTotalBytes        int64 `json:"max_total_bytes"`
	DownloadBufferBytes  int   `json:"download_buffer_bytes"`
	RequestTimeoutSecond int   `json:"request_timeout_seconds"`
}

type source struct {
	ID          string          `json:"id"`
	Repository  string          `json:"repository"`
	Tag         string          `json:"tag,omitempty"`
	Commit      string          `json:"commit"`
	Destination string          `json:"destination"`
	License     json.RawMessage `json:"license"`
	Files       []sourceFile    `json:"files"`
}

type sourceFile struct {
	UpstreamPath string `json:"upstream_path"`
	InstallPath  string `json:"install_path"`
	URL          string `json:"url"`
	SHA256       string `json:"sha256"`
	SizeBytes    int64  `json:"size_bytes"`
}

func main() {
	manifestPath := flag.String("manifest", "", "pinned source manifest")
	destination := flag.String("out", "", "local-only bundle destination")
	flag.Parse()
	if err := run(*manifestPath, *destination, os.Stdout); err != nil {
		fmt.Fprintln(os.Stderr, "datafetch:", err)
		os.Exit(1)
	}
}

func run(manifestPath, destination string, output io.Writer) error {
	if manifestPath == "" || destination == "" {
		return errors.New("both -manifest and -out are required")
	}
	raw, err := os.ReadFile(manifestPath)
	if err != nil {
		return fmt.Errorf("read source manifest: %w", err)
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	var document manifest
	if err := decoder.Decode(&document); err != nil {
		return fmt.Errorf("decode source manifest: %w", err)
	}
	if err := decoder.Decode(&struct{}{}); !errors.Is(err, io.EOF) {
		return errors.New("decode source manifest: trailing JSON value")
	}
	destination, err = validateManifest(&document, destination)
	if err != nil {
		return err
	}
	if info, statErr := os.Lstat(destination); statErr == nil {
		if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
			return fmt.Errorf("destination is not a real directory: %s", destination)
		}
		if err := requireRealDirectory(destination); err != nil {
			return err
		}
		if err := verifyBundle(&document, destination); err != nil {
			return err
		}
		writePaths(output, &document, destination, true)
		return nil
	} else if !os.IsNotExist(statErr) {
		return fmt.Errorf("inspect destination: %w", statErr)
	}

	cacheRoot := destination + ".downloads"
	if err := makeRealDirectory(cacheRoot); err != nil {
		return fmt.Errorf("create download cache: %w", err)
	}
	staging := fmt.Sprintf("%s.installing-%d", destination, os.Getpid())
	if _, err := os.Lstat(staging); err == nil {
		return fmt.Errorf("staging path already exists: %s", staging)
	} else if !os.IsNotExist(err) {
		return fmt.Errorf("inspect staging path: %w", err)
	}
	if err := makeRealDirectory(staging); err != nil {
		return fmt.Errorf("create staging bundle: %w", err)
	}
	published := false
	defer func() {
		if !published {
			_ = os.RemoveAll(staging)
		}
	}()

	for _, source := range document.Sources {
		sourceRoot, _ := safeJoin(staging, source.Destination)
		if err := makeRealDirectory(sourceRoot); err != nil {
			return fmt.Errorf("create source staging directory: %w", err)
		}
		cacheSource, _ := safeJoin(cacheRoot, source.ID)
		if err := makeRealDirectory(cacheSource); err != nil {
			return fmt.Errorf("create source cache: %w", err)
		}
		for index, file := range source.Files {
			cacheName := fmt.Sprintf("%02d-%s", index, filepath.Base(filepath.FromSlash(file.InstallPath)))
			cachePath, _ := safeJoin(cacheSource, cacheName)
			if err := acquire(file, cachePath, document.Limits); err != nil {
				return fmt.Errorf("source %s/%s: %w", source.ID, file.InstallPath, err)
			}
			installed, _ := safeJoin(sourceRoot, file.InstallPath)
			if err := makeRealDirectory(filepath.Dir(installed)); err != nil {
				return fmt.Errorf("create installed parent: %w", err)
			}
			if err := copyFile(cachePath, installed); err != nil {
				return fmt.Errorf("stage %s/%s: %w", source.ID, file.InstallPath, err)
			}
		}
	}
	if err := verifyBundle(&document, staging); err != nil {
		return err
	}
	if err := makeRealDirectory(filepath.Dir(destination)); err != nil {
		return fmt.Errorf("create destination parent: %w", err)
	}
	if err := os.Rename(staging, destination); err != nil {
		if _, statErr := os.Stat(destination); statErr == nil {
			if verifyErr := verifyBundle(&document, destination); verifyErr != nil {
				return fmt.Errorf("concurrent destination is invalid: %w", verifyErr)
			}
		} else {
			return fmt.Errorf("publish bundle: %w", err)
		}
	} else {
		published = true
	}
	writePaths(output, &document, destination, false)
	return nil
}

func validateManifest(document *manifest, destination string) (string, error) {
	if document.Schema != 1 || document.ArtifactPolicy != "local-only" {
		return "", errors.New("source manifest schema or local-only policy is invalid")
	}
	if document.Protocol.GameVersion != "1.26.30" || document.Protocol.ProtocolVersion != 1001 {
		return "", errors.New("source manifest is not Bedrock 1.26.30 / protocol 1001")
	}
	limits := document.Limits
	if limits.MaxSources < 1 || limits.MaxSources > 64 || len(document.Sources) < 1 || len(document.Sources) > limits.MaxSources ||
		limits.MaxFilesPerSource < 1 || limits.MaxFilesPerSource > 1024 || limits.MaxFileBytes < 1 ||
		limits.MaxFileBytes > 1<<30 || limits.MaxTotalBytes < limits.MaxFileBytes || limits.MaxTotalBytes > 1<<32 ||
		limits.DownloadBufferBytes < 4096 || limits.DownloadBufferBytes > 1<<20 ||
		limits.RequestTimeoutSecond < 1 || limits.RequestTimeoutSecond > 300 {
		return "", errors.New("source manifest limits are invalid")
	}
	abs, err := filepath.Abs(destination)
	if err != nil {
		return "", fmt.Errorf("resolve destination: %w", err)
	}
	if filepath.Clean(abs) == filepath.Clean(filepath.VolumeName(abs)+string(filepath.Separator)) {
		return "", errors.New("destination cannot be a filesystem root")
	}
	seenIDs, seenDestinations := map[string]bool{}, map[string]bool{}
	var total int64
	for _, source := range document.Sources {
		if !sourceIDPattern.MatchString(source.ID) || seenIDs[source.ID] {
			return "", fmt.Errorf("source id is invalid or duplicated: %s", source.ID)
		}
		seenIDs[source.ID] = true
		repository, err := url.Parse(source.Repository)
		if err != nil || repository.Scheme != "https" || repository.Host == "" {
			return "", fmt.Errorf("source %s repository must use HTTPS", source.ID)
		}
		if !commitPattern.MatchString(source.Commit) {
			return "", fmt.Errorf("source %s commit must be a lowercase full SHA", source.ID)
		}
		if len(source.License) == 0 || string(source.License) == "null" {
			return "", fmt.Errorf("source %s license evidence is missing", source.ID)
		}
		destinationPath, err := safeJoin(abs, source.Destination)
		if err != nil || seenDestinations[destinationPath] {
			return "", fmt.Errorf("source destination is invalid or duplicated: %s", source.Destination)
		}
		seenDestinations[destinationPath] = true
		if len(source.Files) < 1 || len(source.Files) > limits.MaxFilesPerSource {
			return "", fmt.Errorf("source %s file count is invalid", source.ID)
		}
		seenFiles := map[string]bool{}
		for _, file := range source.Files {
			installed, err := safeJoin(destinationPath, file.InstallPath)
			if err != nil || seenFiles[installed] {
				return "", fmt.Errorf("source %s install path is invalid or duplicated: %s", source.ID, file.InstallPath)
			}
			seenFiles[installed] = true
			if file.SizeBytes < 1 || file.SizeBytes > limits.MaxFileBytes || total > limits.MaxTotalBytes-file.SizeBytes {
				return "", fmt.Errorf("source %s/%s exceeds byte limits", source.ID, file.InstallPath)
			}
			total += file.SizeBytes
			decoded, err := hex.DecodeString(file.SHA256)
			if err != nil || len(decoded) != sha256.Size || strings.ToLower(file.SHA256) != file.SHA256 {
				return "", fmt.Errorf("source %s/%s has invalid SHA-256", source.ID, file.InstallPath)
			}
			parsed, err := url.Parse(file.URL)
			if err != nil || (parsed.Scheme != "https" && parsed.Scheme != "file") {
				return "", fmt.Errorf("source %s/%s URL must use HTTPS", source.ID, file.InstallPath)
			}
		}
	}
	return abs, nil
}

func acquire(file sourceFile, cachePath string, limits limits) error {
	if err := verifyFile(cachePath, file); err == nil {
		return nil
	}
	partial := fmt.Sprintf("%s.partial-%d", cachePath, os.Getpid())
	_ = os.Remove(partial)
	defer os.Remove(partial)
	reader, closeReader, err := openSource(file.URL, time.Duration(limits.RequestTimeoutSecond)*time.Second)
	if err != nil {
		return err
	}
	defer closeReader()
	out, err := os.OpenFile(partial, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o600)
	if err != nil {
		return err
	}
	written, copyErr := io.CopyBuffer(out, io.LimitReader(reader, file.SizeBytes+1), make([]byte, limits.DownloadBufferBytes))
	closeErr := out.Close()
	if copyErr != nil {
		return copyErr
	}
	if closeErr != nil {
		return closeErr
	}
	if written != file.SizeBytes {
		return fmt.Errorf("size mismatch: expected %d, got %d", file.SizeBytes, written)
	}
	if err := verifyFile(partial, file); err != nil {
		return err
	}
	if err := os.Rename(partial, cachePath); err != nil {
		_ = os.Remove(cachePath)
		if retryErr := os.Rename(partial, cachePath); retryErr != nil {
			return retryErr
		}
	}
	return nil
}

func openSource(raw string, timeout time.Duration) (io.Reader, func(), error) {
	parsed, err := url.Parse(raw)
	if err != nil {
		return nil, func() {}, err
	}
	if parsed.Scheme == "file" {
		path := filepath.FromSlash(parsed.Path)
		if runtime.GOOS == "windows" && len(path) >= 3 && path[0] == filepath.Separator && path[2] == ':' {
			path = path[1:]
		}
		file, err := os.Open(path)
		if err != nil {
			return nil, func() {}, err
		}
		return file, func() { _ = file.Close() }, nil
	}
	client := &http.Client{Timeout: timeout}
	response, err := client.Get(raw)
	if err != nil {
		return nil, func() {}, err
	}
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		response.Body.Close()
		return nil, func() {}, fmt.Errorf("HTTP status %s", response.Status)
	}
	return response.Body, func() { _ = response.Body.Close() }, nil
}

func verifyBundle(document *manifest, root string) error {
	expected := map[string]sourceFile{}
	for _, source := range document.Sources {
		for _, file := range source.Files {
			relative := filepath.Join(filepath.FromSlash(source.Destination), filepath.FromSlash(file.InstallPath))
			expected[filepath.Clean(relative)] = file
		}
	}
	actual := make([]string, 0, len(expected))
	err := filepath.WalkDir(root, func(path string, entry os.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if entry.Type()&os.ModeSymlink != 0 {
			return fmt.Errorf("bundle contains symlink: %s", path)
		}
		if entry.IsDir() {
			return nil
		}
		relative, err := filepath.Rel(root, path)
		if err != nil {
			return err
		}
		file, ok := expected[filepath.Clean(relative)]
		if !ok {
			return fmt.Errorf("bundle contains unexpected file: %s", relative)
		}
		if err := verifyFile(path, file); err != nil {
			return fmt.Errorf("verify installed %s: %w", relative, err)
		}
		actual = append(actual, filepath.Clean(relative))
		return nil
	})
	if err != nil {
		return err
	}
	if len(actual) != len(expected) {
		return fmt.Errorf("bundle file count mismatch: expected %d, got %d", len(expected), len(actual))
	}
	for _, source := range document.Sources {
		if source.ID == "pmmp-bedrock-data" {
			path := filepath.Join(root, filepath.FromSlash(source.Destination), "protocol_info.json")
			if err := verifyPMMP(path, document.Protocol); err != nil {
				return err
			}
		}
	}
	return nil
}

func verifyFile(path string, expected sourceFile) error {
	info, err := os.Lstat(path)
	if err != nil {
		return err
	}
	if !info.Mode().IsRegular() || info.Mode()&os.ModeSymlink != 0 || info.Size() != expected.SizeBytes {
		return fmt.Errorf("file size or type mismatch: %s", path)
	}
	file, err := os.Open(path)
	if err != nil {
		return err
	}
	defer file.Close()
	digest := sha256.New()
	if _, err := io.Copy(digest, file); err != nil {
		return err
	}
	if hex.EncodeToString(digest.Sum(nil)) != expected.SHA256 {
		return fmt.Errorf("SHA-256 mismatch: %s", path)
	}
	return nil
}

func verifyPMMP(path string, expected protocol) error {
	bytes, err := os.ReadFile(path)
	if err != nil {
		return fmt.Errorf("read PMMP protocol metadata: %w", err)
	}
	var info struct {
		Version struct {
			Major           int `json:"major"`
			Minor           int `json:"minor"`
			Patch           int `json:"patch"`
			ProtocolVersion int `json:"protocol_version"`
		} `json:"version"`
	}
	if err := json.Unmarshal(bytes, &info); err != nil {
		return fmt.Errorf("decode PMMP protocol metadata: %w", err)
	}
	version := fmt.Sprintf("%d.%d.%d", info.Version.Major, info.Version.Minor, info.Version.Patch)
	if version != expected.GameVersion || info.Version.ProtocolVersion != expected.ProtocolVersion {
		return fmt.Errorf("PMMP protocol metadata mismatch: %s / %d", version, info.Version.ProtocolVersion)
	}
	return nil
}

func safeJoin(root, relative string) (string, error) {
	if relative == "" || filepath.IsAbs(relative) || strings.Contains(relative, `\`) {
		return "", errors.New("relative path is empty, absolute, or noncanonical")
	}
	clean := filepath.Clean(filepath.FromSlash(relative))
	if clean == "." || clean == ".." || strings.HasPrefix(clean, ".."+string(filepath.Separator)) {
		return "", errors.New("relative path escapes its root")
	}
	joined := filepath.Join(root, clean)
	rel, err := filepath.Rel(root, joined)
	if err != nil || rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
		return "", errors.New("relative path escapes its root")
	}
	return joined, nil
}

func makeRealDirectory(path string) error {
	abs, err := filepath.Abs(path)
	if err != nil {
		return err
	}
	ancestor := abs
	for {
		_, err := os.Lstat(ancestor)
		if err == nil {
			if err := requireRealDirectory(ancestor); err != nil {
				return err
			}
			break
		}
		if !os.IsNotExist(err) {
			return err
		}
		parent := filepath.Dir(ancestor)
		if parent == ancestor {
			return fmt.Errorf("no existing directory ancestor for %s", abs)
		}
		ancestor = parent
	}
	if err := os.MkdirAll(abs, 0o755); err != nil {
		return err
	}
	return requireRealDirectory(abs)
}

func requireRealDirectory(path string) error {
	abs, err := filepath.Abs(path)
	if err != nil {
		return err
	}
	info, err := os.Lstat(abs)
	if err != nil {
		return err
	}
	if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		return fmt.Errorf("directory is a symlink, junction, or non-directory: %s", abs)
	}
	resolved, err := filepath.EvalSymlinks(abs)
	if err != nil {
		return err
	}
	resolved, err = filepath.Abs(resolved)
	if err != nil {
		return err
	}
	equal := filepath.Clean(abs) == filepath.Clean(resolved)
	if runtime.GOOS == "windows" {
		equal = strings.EqualFold(filepath.Clean(abs), filepath.Clean(resolved))
	}
	if !equal {
		return fmt.Errorf("directory resolves through a symlink or junction: %s", abs)
	}
	return nil
}

func copyFile(source, destination string) error {
	in, err := os.Open(source)
	if err != nil {
		return err
	}
	defer in.Close()
	out, err := os.OpenFile(destination, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o600)
	if err != nil {
		return err
	}
	_, copyErr := io.Copy(out, in)
	closeErr := out.Close()
	if copyErr != nil {
		return copyErr
	}
	return closeErr
}

func writePaths(output io.Writer, document *manifest, root string, verified bool) {
	if output == nil {
		return
	}
	sources := append([]source(nil), document.Sources...)
	sort.Slice(sources, func(i, j int) bool { return sources[i].ID < sources[j].ID })
	for _, source := range sources {
		path := filepath.Join(root, filepath.FromSlash(source.Destination))
		if verified {
			fmt.Fprintf(output, "%s already verified: %s\n", source.ID, path)
		}
		fmt.Fprintf(output, "SOURCE_PATH %s=%s\n", source.ID, path)
	}
}
