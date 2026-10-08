package catalog

import (
	_ "embed"
	"strconv"
	"strings"
)

//go:embed artwork_limit.txt
var artworkByteLimit string

// The account manager uses this same bound when preserving downloaded gamer pictures.
var maxArtworkBytes = func() int {
	limit, err := strconv.Atoi(strings.TrimSpace(artworkByteLimit))
	if err != nil || limit <= 0 {
		panic("invalid embedded artwork byte limit")
	}
	return limit
}()
