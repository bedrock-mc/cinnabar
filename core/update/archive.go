package update

import (
	"archive/tar"
	"compress/gzip"
	"errors"
	"fmt"
	"io"
	"os"
	"path"
	"path/filepath"
	"strings"
)

// extractApp expands only a single app bundle; links cannot escape it or redirect writes.
func extractApp(archive, directory string) (string, error) {
	file, err := os.Open(archive)
	if err != nil {
		return "", err
	}
	defer file.Close()
	compressed, err := gzip.NewReader(file)
	if err != nil {
		return "", err
	}
	defer compressed.Close()
	reader := tar.NewReader(compressed)
	root := ""
	links := map[string]string{}
	var expanded int64
	for {
		header, err := reader.Next()
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			return "", err
		}
		name := path.Clean(strings.TrimPrefix(header.Name, "./"))
		if name == "." {
			continue
		}
		if strings.Contains(name, "\\") || strings.HasPrefix(name, "/") || name == ".." || strings.HasPrefix(name, "../") {
			return "", errors.New("unsafe archive path")
		}
		first := strings.Split(name, "/")[0]
		if !strings.HasSuffix(first, ".app") || (root != "" && root != first) {
			return "", errors.New("archive must contain exactly one app bundle")
		}
		root = first
		target := filepath.Join(directory, filepath.FromSlash(name))
		switch header.Typeflag {
		case tar.TypeDir:
			if err := os.MkdirAll(target, 0o755); err != nil {
				return "", err
			}
		case tar.TypeReg, tar.TypeRegA:
			if header.Size < 0 || header.Size > (8<<30)-expanded {
				return "", errors.New("expanded update exceeds size limit")
			}
			expanded += header.Size
			if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
				return "", err
			}
			out, err := os.OpenFile(target, os.O_CREATE|os.O_EXCL|os.O_WRONLY, os.FileMode(header.Mode)&0o755)
			if err != nil {
				return "", err
			}
			_, copyErr := io.Copy(out, reader)
			closeErr := out.Close()
			if err := errors.Join(copyErr, closeErr); err != nil {
				return "", err
			}
		case tar.TypeSymlink:
			link := header.Linkname
			resolved := path.Clean(path.Join(path.Dir(name), link))
			if path.IsAbs(link) || strings.Contains(link, "\\") || !strings.HasPrefix(resolved, root+"/") {
				return "", errors.New("unsafe archive symlink")
			}
			links[target] = link
		default:
			return "", fmt.Errorf("unsupported archive entry type %d", header.Typeflag)
		}
	}
	if root == "" {
		return "", errors.New("empty app archive")
	}
	for target, link := range links {
		for parent := filepath.Dir(target); parent != directory && parent != "."; parent = filepath.Dir(parent) {
			if _, ok := links[parent]; ok {
				return "", errors.New("archive symlink cannot contain another entry")
			}
		}
		if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
			return "", err
		}
		if err := os.Symlink(link, target); err != nil {
			return "", err
		}
	}
	bundle := filepath.Join(directory, root)
	canonicalBundle, err := filepath.EvalSymlinks(bundle)
	if err != nil {
		return "", err
	}
	for target := range links {
		resolved, err := filepath.EvalSymlinks(target)
		if err != nil {
			return "", fmt.Errorf("invalid archive symlink: %w", err)
		}
		relative, err := filepath.Rel(canonicalBundle, resolved)
		if err != nil || relative == ".." || strings.HasPrefix(relative, ".."+string(filepath.Separator)) || filepath.IsAbs(relative) {
			return "", errors.New("archive symlink escapes the app bundle")
		}
	}
	if info, err := os.Stat(filepath.Join(bundle, "Contents", "MacOS", "bedrock-client")); err != nil || !info.Mode().IsRegular() || info.Mode()&0o111 == 0 {
		return "", errors.New("archive is missing the executable app entry point")
	}
	return bundle, nil
}
