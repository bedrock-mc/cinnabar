//go:build !windows

package main

// normalUser leaves other platforms' process permissions unchanged.
func normalUser() error { return nil }
