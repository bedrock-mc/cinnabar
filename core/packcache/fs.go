package packcache

import "os"

// regularNoLink accepts archive files and rejects directories and links.
func regularNoLink(info os.FileInfo) bool { return info.Mode().IsRegular() && !hasLinkAttribute(info) }
