//go:build !windows

package coreapp

// normalUser leaves other platforms' process permissions unchanged.
func normalUser() error { return nil }
