package authcache

import (
	_ "embed"
	"strconv"
	"strings"
)

//go:embed cache_limit.txt
var cacheLimitText string

// maxCacheSize is shared with Rust's active OAuth writer, independently of account wrappers.
var maxCacheSize = cacheByteLimit(cacheLimitText)

// cacheByteLimit rejects an invalid embedded policy before a cache can be read or written.
func cacheByteLimit(value string) int {
	limit, err := strconv.Atoi(strings.TrimSpace(value))
	if err != nil || limit <= 0 {
		panic("invalid embedded credential cache byte limit")
	}
	return limit
}
