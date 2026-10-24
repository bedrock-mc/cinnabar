//go:build !linux

package packcache

import "os"

// publishNoReplace publishes a completed archive without overwriting another entry.
func publishNoReplace(temp, dest string) error {
	if err := os.Link(temp, dest); err != nil {
		return err
	}
	return os.Remove(temp)
}
