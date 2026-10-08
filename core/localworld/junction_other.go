//go:build !windows

package localworld

import "os"

func createWorldLink(link, target string) error { return os.Symlink(target, link) }
