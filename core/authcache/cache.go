package authcache

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"sync"

	"github.com/sandertv/gophertunnel/minecraft/auth"
	"golang.org/x/oauth2"
)

const maxCacheSize = 64 * 1024

// Config configures a checked Microsoft token cache.
type Config struct {
	Path    string
	Writer  io.Writer
	Request func(context.Context, io.Writer) (*oauth2.Token, error)
	Refresh func(*oauth2.Token, io.Writer) oauth2.TokenSource
}

// Source loads or acquires a Microsoft token and returns a source that persists
// each successfully refreshed token before returning it to the caller.
func Source(ctx context.Context, config Config) (oauth2.TokenSource, error) {
	if config.Path == "" {
		return nil, errors.New("auth cache path is empty")
	}
	path, err := filepath.Abs(config.Path)
	if err != nil {
		return nil, errors.New("resolve auth cache path")
	}
	config.Path = filepath.Clean(path)
	writer := config.Writer
	if writer == nil {
		writer = io.Discard
	}
	request := config.Request
	if request == nil {
		request = auth.AndroidConfig.RequestLiveTokenContext
	}
	refresh := config.Refresh
	if refresh == nil {
		refresh = auth.AndroidConfig.RefreshTokenSourceWriter
	}

	cached, err := load(config.Path)
	if err != nil {
		if !errors.Is(err, fs.ErrNotExist) {
			return nil, fmt.Errorf("load Microsoft auth cache: %w", err)
		}
		return acquire(ctx, config.Path, writer, request, refresh)
	}

	source := refresh(cached, writer)
	if source == nil {
		return acquire(ctx, config.Path, writer, request, refresh)
	}
	current, err := source.Token()
	if err != nil || !validToken(current) {
		return acquire(ctx, config.Path, writer, request, refresh)
	}
	if err := save(config.Path, current); err != nil {
		return nil, fmt.Errorf("persist refreshed Microsoft token: %w", err)
	}
	return &persistingSource{path: config.Path, source: source}, nil
}

func acquire(
	ctx context.Context,
	path string,
	writer io.Writer,
	request func(context.Context, io.Writer) (*oauth2.Token, error),
	refresh func(*oauth2.Token, io.Writer) oauth2.TokenSource,
) (oauth2.TokenSource, error) {
	parents, err := snapshotDirectoryChain(filepath.Dir(path))
	if err != nil {
		return nil, err
	}
	if err := parents.revalidate(); err != nil {
		return nil, err
	}
	tok, err := request(ctx, writer)
	if err != nil {
		return nil, fmt.Errorf("request Microsoft token: %w", err)
	}
	if err := parents.revalidate(); err != nil {
		return nil, err
	}
	if !validToken(tok) {
		return nil, errors.New("request Microsoft token: token has no refresh token")
	}
	if err := save(path, tok); err != nil {
		return nil, fmt.Errorf("persist Microsoft token: %w", err)
	}
	source := refresh(tok, writer)
	if source == nil {
		return nil, errors.New("create Microsoft refresh source: nil token source")
	}
	return &persistingSource{path: path, source: source}, nil
}

type persistingSource struct {
	mu     sync.Mutex
	path   string
	source oauth2.TokenSource
}

func (s *persistingSource) Token() (*oauth2.Token, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	tok, err := s.source.Token()
	if err != nil {
		return nil, err
	}
	if !validToken(tok) {
		return nil, errors.New("refresh Microsoft token: token has no refresh token")
	}
	if err := save(s.path, tok); err != nil {
		return nil, fmt.Errorf("persist refreshed Microsoft token: %w", err)
	}
	return tok, nil
}

func load(path string) (*oauth2.Token, error) {
	initialParents, err := snapshotDirectoryChain(filepath.Dir(path))
	if err != nil {
		return nil, err
	}
	pathInfo, err := os.Lstat(path)
	if err != nil {
		return nil, err
	}
	if err := checkRegular(pathInfo); err != nil {
		return nil, err
	}
	if pathInfo.Size() > maxCacheSize {
		return nil, fmt.Errorf("auth cache exceeds %d bytes", maxCacheSize)
	}
	parents, err := snapshotDirectoryChain(filepath.Dir(path))
	if err != nil {
		return nil, err
	}
	if !parents.complete {
		return nil, errors.New("auth cache parent changed while opening")
	}
	if err := initialParents.revalidate(); err != nil {
		return nil, err
	}

	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()

	openInfo, err := file.Stat()
	if err != nil {
		return nil, err
	}
	if err := checkRegular(openInfo); err != nil {
		return nil, err
	}
	if !os.SameFile(pathInfo, openInfo) {
		return nil, errors.New("auth cache changed while opening")
	}
	if err := parents.revalidate(); err != nil {
		return nil, err
	}
	if openInfo.Size() > maxCacheSize {
		return nil, fmt.Errorf("auth cache exceeds %d bytes", maxCacheSize)
	}

	contents, err := io.ReadAll(io.LimitReader(file, maxCacheSize+1))
	if err != nil {
		return nil, err
	}
	if len(contents) > maxCacheSize {
		return nil, fmt.Errorf("auth cache exceeds %d bytes", maxCacheSize)
	}
	finalInfo, err := file.Stat()
	if err != nil {
		return nil, err
	}
	if finalInfo.Size() > maxCacheSize || finalInfo.Size() != int64(len(contents)) {
		return nil, errors.New("auth cache changed while reading")
	}
	if err := parents.revalidate(); err != nil {
		return nil, err
	}

	decoder := json.NewDecoder(bytes.NewReader(contents))
	var tok oauth2.Token
	if err := decoder.Decode(&tok); err != nil {
		return nil, fmt.Errorf("decode auth cache: %w", err)
	}
	var trailing any
	if err := decoder.Decode(&trailing); !errors.Is(err, io.EOF) {
		if err == nil {
			return nil, errors.New("decode auth cache: trailing JSON value")
		}
		return nil, fmt.Errorf("decode auth cache trailing data: %w", err)
	}
	if !validToken(&tok) {
		return nil, errors.New("decode auth cache: token has no refresh token")
	}
	return &tok, nil
}

func save(path string, tok *oauth2.Token) error {
	return saveWithHooks(path, tok, saveHooks{})
}

type saveHooks struct {
	afterTokenSync            func(tempPath string) error
	scrubTemp                 func(*os.File) error
	afterCleanupIdentityCheck func(tempPath string)
}

func saveWithHooks(path string, tok *oauth2.Token, hooks saveHooks) (returnErr error) {
	serialized, err := serializeToken(tok)
	if err != nil {
		return err
	}
	dir := filepath.Dir(path)
	beforeCreate, err := snapshotDirectoryChain(dir)
	if err != nil {
		return err
	}
	if err := beforeCreate.revalidate(); err != nil {
		return err
	}
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return err
	}
	if err := beforeCreate.revalidate(); err != nil {
		return err
	}
	parents, err := snapshotDirectoryChain(dir)
	if err != nil {
		return err
	}
	if !parents.complete {
		return errors.New("auth cache parent was not created")
	}
	root, err := os.OpenRoot(dir)
	if err != nil {
		return errors.New("open auth cache parent")
	}
	defer root.Close()
	rootInfo, err := root.Stat(".")
	if err != nil || len(parents.directories) == 0 || !os.SameFile(parents.directories[len(parents.directories)-1].info, rootInfo) {
		return errors.New("auth cache parent changed while opening")
	}

	targetName := filepath.Base(path)
	original, originalExists, err := publicationTargetAt(root, targetName)
	if err != nil {
		return err
	}
	if err := parents.revalidate(); err != nil {
		return err
	}
	file, tempName, err := createRootTemp(root)
	if err != nil {
		return err
	}
	tempPath := filepath.Join(dir, tempName)
	tempInfo, err := file.Stat()
	if err != nil {
		_ = file.Close()
		return err
	}
	success := false
	defer func() {
		if success {
			return
		}
		if err := cleanupTempIdentity(root, file, tempInfo, hooks); err != nil {
			returnErr = errors.New("secure auth cache cleanup failed")
		}
	}()
	if err := checkRegular(tempInfo); err != nil {
		return err
	}
	if err := parents.revalidate(); err != nil {
		return err
	}

	if err := file.Chmod(0o600); err != nil {
		return err
	}
	written, err := file.Write(serialized)
	if err != nil {
		return err
	}
	if written != len(serialized) {
		return io.ErrShortWrite
	}
	if err := file.Sync(); err != nil {
		return err
	}
	if hooks.afterTokenSync != nil {
		if err := hooks.afterTokenSync(tempPath); err != nil {
			return errors.New("auth cache parent changed after writing")
		}
	}
	if err := parents.revalidate(); err != nil {
		return err
	}
	currentTempInfo, err := root.Lstat(tempName)
	if err != nil {
		return err
	}
	if err := checkRegular(currentTempInfo); err != nil {
		return err
	}
	if !os.SameFile(tempInfo, currentTempInfo) {
		return errors.New("temporary auth cache changed while writing")
	}
	if err := parents.revalidate(); err != nil {
		return err
	}

	current, currentExists, err := publicationTargetAt(root, targetName)
	if err != nil {
		return err
	}
	if originalExists != currentExists || (originalExists && !os.SameFile(original, current)) {
		return errors.New("auth cache changed before publication")
	}
	if err := parents.revalidate(); err != nil {
		return err
	}
	if err := root.Rename(tempName, targetName); err != nil {
		return err
	}
	if err := parents.revalidate(); err != nil {
		return err
	}
	published, err := root.Lstat(targetName)
	if err != nil {
		return err
	}
	if err := checkRegular(published); err != nil {
		return err
	}
	if !os.SameFile(tempInfo, published) {
		return errors.New("auth cache changed during publication")
	}
	if err := file.Close(); err != nil {
		return err
	}
	success = true
	return nil
}

func serializeToken(tok *oauth2.Token) ([]byte, error) {
	if !validToken(tok) {
		return nil, errors.New("refusing to persist token without refresh token")
	}
	remaining := maxCacheSize
	for _, field := range []string{tok.AccessToken, tok.TokenType, tok.RefreshToken} {
		if len(field) > remaining {
			return nil, fmt.Errorf("auth cache exceeds %d bytes", maxCacheSize)
		}
		remaining -= len(field)
	}
	serialized, err := json.Marshal(tok)
	if err != nil {
		return nil, err
	}
	if len(serialized) >= maxCacheSize {
		return nil, fmt.Errorf("auth cache exceeds %d bytes", maxCacheSize)
	}
	return append(serialized, '\n'), nil
}

func createRootTemp(root *os.Root) (*os.File, string, error) {
	for range 100 {
		var random [16]byte
		if _, err := rand.Read(random[:]); err != nil {
			return nil, "", errors.New("generate temporary auth cache name")
		}
		name := ".microsoft-token-" + hex.EncodeToString(random[:]) + ".tmp"
		file, err := root.OpenFile(name, os.O_RDWR|os.O_CREATE|os.O_EXCL, 0o600)
		if errors.Is(err, fs.ErrExist) {
			continue
		}
		if err != nil {
			return nil, "", errors.New("create temporary auth cache")
		}
		return file, name, nil
	}
	return nil, "", errors.New("create unique temporary auth cache")
}

func cleanupTempIdentity(root *os.Root, file *os.File, identity fs.FileInfo, hooks saveHooks) error {
	scrub := scrubOpenTemp
	if hooks.scrubTemp != nil {
		scrub = hooks.scrubTemp
	}
	scrubErr := scrub(file)
	closeErr := file.Close()
	names, scanErr := identityNames(root, identity)
	if scanErr == nil && hooks.afterCleanupIdentityCheck != nil && len(names) != 0 {
		hooks.afterCleanupIdentityCheck(filepath.Join(root.Name(), names[0]))
	}
	_, rescanErr := identityNames(root, identity)
	if scrubErr != nil || closeErr != nil || scanErr != nil || rescanErr != nil {
		return errors.New("temporary auth cache cleanup could not be verified")
	}
	return nil
}

func scrubOpenTemp(file *os.File) error {
	var scrubErr error
	if _, err := file.Seek(0, io.SeekStart); err != nil {
		scrubErr = err
	}
	if err := file.Truncate(0); err != nil {
		scrubErr = err
	}
	if err := file.Sync(); err != nil {
		scrubErr = err
	}
	return scrubErr
}

func identityNames(root *os.Root, identity fs.FileInfo) ([]string, error) {
	dir, err := root.Open(".")
	if err != nil {
		return nil, err
	}
	entries, readErr := dir.ReadDir(-1)
	closeErr := dir.Close()
	if readErr != nil {
		return nil, readErr
	}
	if closeErr != nil {
		return nil, closeErr
	}
	var names []string
	for _, entry := range entries {
		info, err := root.Lstat(entry.Name())
		if errors.Is(err, fs.ErrNotExist) {
			continue
		}
		if err != nil {
			return nil, err
		}
		if !os.SameFile(identity, info) {
			continue
		}
		if err := checkRegular(info); err != nil {
			return nil, err
		}
		names = append(names, entry.Name())
	}
	return names, nil
}

type directoryIdentity struct {
	path string
	info fs.FileInfo
}

type directoryChain struct {
	directories []directoryIdentity
	complete    bool
}

func snapshotDirectoryChain(dir string) (directoryChain, error) {
	if !filepath.IsAbs(dir) {
		return directoryChain{}, errors.New("auth cache parent is not absolute")
	}
	paths := ancestorPaths(filepath.Clean(dir))
	chain := directoryChain{directories: make([]directoryIdentity, 0, len(paths))}
	for _, path := range paths {
		info, err := os.Lstat(path)
		if errors.Is(err, fs.ErrNotExist) {
			return chain, nil
		}
		if err != nil {
			return directoryChain{}, errors.New("inspect auth cache parent")
		}
		if err := checkDirectory(info); err != nil {
			return directoryChain{}, err
		}
		chain.directories = append(chain.directories, directoryIdentity{path: path, info: info})
	}
	chain.complete = true
	return chain, nil
}

func ancestorPaths(path string) []string {
	var reversed []string
	for {
		reversed = append(reversed, path)
		parent := filepath.Dir(path)
		if parent == path {
			break
		}
		path = parent
	}
	paths := make([]string, len(reversed))
	for i := range reversed {
		paths[len(reversed)-1-i] = reversed[i]
	}
	return paths
}

func (chain directoryChain) revalidate() error {
	for _, directory := range chain.directories {
		info, err := os.Lstat(directory.path)
		if err != nil {
			return errors.New("auth cache parent changed")
		}
		if err := checkDirectory(info); err != nil {
			return err
		}
		if !os.SameFile(directory.info, info) {
			return errors.New("auth cache parent changed")
		}
	}
	return nil
}

func checkDirectory(info fs.FileInfo) error {
	if info.Mode()&os.ModeSymlink != 0 || isReparsePoint(info) || !info.IsDir() {
		return errors.New("auth cache parent is not a real directory")
	}
	return nil
}

func publicationTargetAt(root *os.Root, name string) (fs.FileInfo, bool, error) {
	info, err := root.Lstat(name)
	if errors.Is(err, fs.ErrNotExist) {
		return nil, false, nil
	}
	if err != nil {
		return nil, false, err
	}
	if err := checkRegular(info); err != nil {
		return nil, false, err
	}
	return info, true, nil
}

func checkRegular(info fs.FileInfo) error {
	if info.Mode()&os.ModeSymlink != 0 || isReparsePoint(info) || !info.Mode().IsRegular() {
		return errors.New("auth cache is not a regular non-link file")
	}
	return nil
}

func validToken(tok *oauth2.Token) bool {
	return tok != nil && tok.RefreshToken != ""
}
