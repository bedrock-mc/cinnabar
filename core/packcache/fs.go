package packcache

import (
	"errors"
	"io/fs"
	"os"
	"path/filepath"
)

func prepareRoot(root string) error {
	parent := filepath.Dir(root)
	if err := validatePathComponents(parent, true); err != nil {
		return err
	}
	_, statErr := os.Lstat(root)
	created := errors.Is(statErr, fs.ErrNotExist)
	if statErr != nil && !created {
		return statErr
	}
	if err := os.MkdirAll(root, 0o700); err != nil {
		return err
	}
	if created {
		if err := secureCreatedPath(root, true); err != nil {
			return err
		}
	}
	return validateRoot(root)
}

func pathExists(path string) bool { _, err := os.Lstat(path); return err == nil }

func canonicalRoot(path string) string {
	path = filepath.Clean(path)
	return canonicalPlatformPath(path)
}

func validateOwnerOnlyPath(path string, directory bool) error {
	info, err := os.Lstat(path)
	if err != nil {
		return err
	}
	if directory {
		if !info.IsDir() || hasLinkAttribute(info) {
			return errors.New("not a real directory")
		}
	} else if !regularNoLink(info) {
		return errors.New("not a regular file")
	}
	if !ownerOnlyPath(path, info) {
		return errors.New("permissions are not owner-only")
	}
	return nil
}

func secureCreatedPath(path string, directory bool) error {
	if err := secureOwnerOnlyPath(path, directory); err != nil {
		return err
	}
	return validateOwnerOnlyPath(path, directory)
}

func validateRoot(root string) error {
	if err := validatePathComponents(root, false); err != nil {
		return err
	}
	info, err := os.Lstat(root)
	if err != nil {
		return err
	}
	if !info.IsDir() || hasLinkAttribute(info) {
		return errors.New("cache root is not a real directory")
	}
	if !ownerOnlyPath(root, info) {
		return errors.New("cache root permissions are not owner-only")
	}
	parentInfo, err := os.Lstat(filepath.Dir(root))
	if err != nil {
		return err
	}
	if !parentInfo.IsDir() || hasLinkAttribute(parentInfo) || !ownerOnlyPath(filepath.Dir(root), parentInfo) {
		return errors.New("cache parent is not an owner-only real directory")
	}
	return nil
}

func validatePathComponents(path string, requireLeafOwnerOnly bool) error {
	volume := filepath.VolumeName(path)
	rest := path[len(volume):]
	current := volume + string(os.PathSeparator)
	parts := splitPath(rest)
	for i, part := range parts {
		current = filepath.Join(current, part)
		info, err := os.Lstat(current)
		if errors.Is(err, fs.ErrNotExist) {
			return nil
		}
		if err != nil {
			return err
		}
		if !info.IsDir() || hasLinkAttribute(info) {
			return errors.New("cache path traverses a linked or non-directory component")
		}
		if requireLeafOwnerOnly && i == len(parts)-1 && !ownerOnlyPath(current, info) {
			return errors.New("cache parent permissions are not owner-only")
		}
	}
	return nil
}

func splitPath(path string) []string {
	var parts []string
	for {
		dir, file := filepath.Split(path)
		if file != "" {
			parts = append([]string{file}, parts...)
		}
		path = filepath.Clean(dir)
		if path == "." || path == string(os.PathSeparator) || path == "" {
			return parts
		}
	}
}

func secureRegular(info os.FileInfo) bool {
	return regularNoLink(info)
}

func regularNoLink(info os.FileInfo) bool { return info.Mode().IsRegular() && !hasLinkAttribute(info) }

func publishNoReplace(temp, dest string) error {
	if err := os.Link(temp, dest); err != nil {
		return err
	}
	return os.Remove(temp)
}
