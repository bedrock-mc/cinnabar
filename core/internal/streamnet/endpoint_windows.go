//go:build windows

package streamnet

import (
	"errors"
	"fmt"
	"os"
)

func validateSocketDirOwner(os.FileInfo) error { return nil }

func validateUnixEndpoint(path string) error {
	return fmt.Errorf("streamnet: Unix endpoints are unavailable on Windows: %s", path)
}

func prepareUnixEndpoint(string) error {
	return errors.New("streamnet: Unix endpoints are unavailable on Windows")
}
func removeUnixEndpoint(string) error { return nil }
