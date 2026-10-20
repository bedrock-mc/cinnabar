package proxy

import (
	_ "embed"
	"strconv"
	"strings"
)

//go:embed resource_pack_window.txt
var resourcePackWindowText string

// embeddedResourcePackWindow reads the shared native and Go acquisition policy.
// Invalid embedded policy is a build configuration error, never server input.
func embeddedResourcePackWindow() int {
	window, err := strconv.ParseUint(strings.TrimRight(resourcePackWindowText, "\r\n"), 10, 31)
	if err != nil || window == 0 {
		panic("invalid embedded resource-pack request window")
	}
	return int(window)
}
