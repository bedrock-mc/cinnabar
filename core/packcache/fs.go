package packcache

import "os"

// regularNoLink accepts archive files and rejects directories and links.
func regularNoLink(info os.FileInfo) bool { return info.Mode().IsRegular() && !hasLinkAttribute(info) }

// publishNoReplace publishes a completed archive without overwriting another entry.
func publishNoReplace(temp, dest string) error {
	if err := os.Link(temp, dest); err != nil {
		return err
	}
	return os.Remove(temp)
}
