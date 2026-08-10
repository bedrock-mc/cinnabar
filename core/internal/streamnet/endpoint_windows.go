//go:build windows

package streamnet

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"syscall"
)

type unixEndpointIdentity struct{}

func unixEndpointPath(socketDir string) string {
	return filepath.Join(socketDir, unixEndpointName)
}

func unixEndpointPathNamed(socketDir, endpointName string) string {
	return filepath.Join(socketDir, endpointName)
}

func validateSocketDirOwner(os.FileInfo) error { return nil }

func validateUnixEndpoint(path string) error {
	return fmt.Errorf("streamnet: Unix endpoints are unavailable on Windows: %s", path)
}

func prepareUnixEndpoint(string) error {
	return errors.New("streamnet: Unix endpoints are unavailable on Windows")
}
func unixEndpointIdentityAt(string) (unixEndpointIdentity, error) {
	return unixEndpointIdentity{}, errors.New("streamnet: Unix endpoints are unavailable on Windows")
}
func removeUnixEndpoint(string, unixEndpointIdentity) error { return nil }

func isConnectionRefused(err error) bool { return errors.Is(err, syscall.Errno(10061)) }
