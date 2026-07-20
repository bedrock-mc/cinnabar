package main

import (
	"os"
	"path/filepath"
	"strings"
	"syscall"
)

func isRedirectingDirectory(info os.FileInfo) bool {
	if info.Mode()&os.ModeSymlink != 0 {
		return true
	}
	attributes, ok := info.Sys().(*syscall.Win32FileAttributeData)
	return ok && attributes.FileAttributes&syscall.FILE_ATTRIBUTE_REPARSE_POINT != 0
}

func sameResolvedDirectoryPath(left, right string) bool {
	left = filepath.Clean(left)
	right = filepath.Clean(right)

	leftRoot, leftParts, ok := windowsDirectoryParts(left)
	if !ok {
		return false
	}
	rightRoot, rightParts, ok := windowsDirectoryParts(right)
	if !ok || !strings.EqualFold(leftRoot, rightRoot) || len(leftParts) != len(rightParts) {
		return false
	}
	leftRootInfo, err := os.Lstat(leftRoot)
	if err != nil || !leftRootInfo.IsDir() || isRedirectingDirectory(leftRootInfo) {
		return false
	}
	rightRootInfo, err := os.Lstat(rightRoot)
	if err != nil || !rightRootInfo.IsDir() || isRedirectingDirectory(rightRootInfo) {
		return false
	}
	if !os.SameFile(leftRootInfo, rightRootInfo) {
		return false
	}

	leftPrefix := leftRoot
	rightPrefix := rightRoot
	for index := range leftParts {
		leftPrefix = filepath.Join(leftPrefix, leftParts[index])
		rightPrefix = filepath.Join(rightPrefix, rightParts[index])
		leftInfo, err := os.Lstat(leftPrefix)
		if err != nil || !leftInfo.IsDir() || isRedirectingDirectory(leftInfo) {
			return false
		}
		rightInfo, err := os.Lstat(rightPrefix)
		if err != nil || !rightInfo.IsDir() || isRedirectingDirectory(rightInfo) {
			return false
		}
		if !os.SameFile(leftInfo, rightInfo) {
			return false
		}
	}
	return true
}

func windowsDirectoryParts(path string) (string, []string, bool) {
	volume := filepath.VolumeName(path)
	if volume == "" {
		return "", nil, false
	}
	root := volume + string(filepath.Separator)
	relative, err := filepath.Rel(root, path)
	if err != nil || filepath.IsAbs(relative) {
		return "", nil, false
	}
	if relative == "." {
		return root, nil, true
	}
	parts := strings.Split(relative, string(filepath.Separator))
	if len(parts) == 0 || slicesContainEmpty(parts) {
		return "", nil, false
	}
	return root, parts, true
}

func slicesContainEmpty(parts []string) bool {
	for _, part := range parts {
		if part == "" || part == "." || part == ".." {
			return true
		}
	}
	return false
}
