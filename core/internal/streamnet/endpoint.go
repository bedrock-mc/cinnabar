package streamnet

import (
	"errors"
	"fmt"
	"net"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"time"
)

const (
	unixEndpointName    = "game.sock"
	windowsEndpointName = "game.addr"
)

// Resolve discovers the local bridge endpoint in socketDir. On Unix it
// returns the fixed Unix-domain socket; on Windows it validates the published
// loopback TCP address.
func Resolve(socketDir string) (network, address string, err error) {
	if socketDir == "" {
		return "", "", errors.New("streamnet: socket directory is empty")
	}
	if runtime.GOOS != "windows" {
		address = filepath.Join(socketDir, unixEndpointName)
		if err := validateUnixEndpoint(address); err != nil {
			return "", "", err
		}
		return "unix", address, nil
	}

	path := filepath.Join(socketDir, windowsEndpointName)
	address, err = readPublishedAddress(path)
	if err != nil {
		return "", "", err
	}
	return "tcp", address, nil
}

func readPublishedAddress(path string) (string, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return "", fmt.Errorf("streamnet: read %s: %w", path, err)
	}
	if len(data) < 2 || len(data) > 128 || data[len(data)-1] != '\n' {
		return "", fmt.Errorf("streamnet: invalid endpoint publication length or terminator")
	}
	address := strings.TrimSuffix(string(data), "\n")
	if address != strings.TrimSpace(address) || strings.ContainsAny(address, "\r\n") {
		return "", fmt.Errorf("streamnet: endpoint publication is not canonical ASCII")
	}
	for _, value := range []byte(address) {
		if value > 0x7f {
			return "", fmt.Errorf("streamnet: endpoint publication is not ASCII")
		}
	}
	host, portText, err := net.SplitHostPort(address)
	if err != nil {
		return "", fmt.Errorf("streamnet: parse published endpoint: %w", err)
	}
	if host != "127.0.0.1" {
		return "", fmt.Errorf("streamnet: published endpoint is not 127.0.0.1: %q", host)
	}
	port, err := strconv.ParseUint(portText, 10, 16)
	if err != nil || port == 0 {
		return "", fmt.Errorf("streamnet: invalid published port %q", portText)
	}
	return net.JoinHostPort(host, strconv.FormatUint(port, 10)), nil
}

func ensureSocketDir(socketDir string) error {
	if socketDir == "" {
		return errors.New("streamnet: socket directory is empty")
	}
	if err := os.MkdirAll(socketDir, 0o700); err != nil {
		return fmt.Errorf("streamnet: create socket directory: %w", err)
	}
	info, err := os.Lstat(socketDir)
	if err != nil {
		return fmt.Errorf("streamnet: inspect socket directory: %w", err)
	}
	if info.Mode()&os.ModeSymlink != 0 || !info.IsDir() {
		return fmt.Errorf("streamnet: socket directory is not a real directory: %s", socketDir)
	}
	if err := validateSocketDirOwner(info); err != nil {
		return err
	}
	if err := os.Chmod(socketDir, 0o700); err != nil {
		return fmt.Errorf("streamnet: secure socket directory: %w", err)
	}
	return nil
}

func publishAddress(socketDir, address string) (string, error) {
	path := filepath.Join(socketDir, windowsEndpointName)
	temp, err := os.CreateTemp(socketDir, windowsEndpointName+".tmp-")
	if err != nil {
		return "", fmt.Errorf("streamnet: create endpoint publication: %w", err)
	}
	tempName := temp.Name()
	defer os.Remove(tempName)
	if err := temp.Chmod(0o600); err != nil {
		_ = temp.Close()
		return "", fmt.Errorf("streamnet: secure endpoint publication: %w", err)
	}
	if _, err := temp.WriteString(address + "\n"); err != nil {
		_ = temp.Close()
		return "", fmt.Errorf("streamnet: write endpoint publication: %w", err)
	}
	if err := temp.Sync(); err != nil {
		_ = temp.Close()
		return "", fmt.Errorf("streamnet: sync endpoint publication: %w", err)
	}
	if err := temp.Close(); err != nil {
		return "", fmt.Errorf("streamnet: close endpoint publication: %w", err)
	}
	if err := os.Rename(tempName, path); err != nil {
		return "", fmt.Errorf("streamnet: publish endpoint: %w", err)
	}
	return path, nil
}

func preparePublishedAddress(socketDir string) error {
	path := filepath.Join(socketDir, windowsEndpointName)
	address, err := readPublishedAddress(path)
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return fmt.Errorf("streamnet: refuse invalid existing endpoint publication: %w", err)
	}
	conn, dialErr := net.DialTimeout("tcp", address, 250*time.Millisecond)
	if dialErr == nil {
		_ = conn.Close()
		return fmt.Errorf("streamnet: endpoint publication is live at %s", address)
	}
	if !isConnectionRefused(dialErr) {
		return fmt.Errorf("streamnet: cannot prove endpoint publication stale: %w", dialErr)
	}
	if err := os.Remove(path); err != nil && !errors.Is(err, os.ErrNotExist) {
		return fmt.Errorf("streamnet: remove stale endpoint publication: %w", err)
	}
	return nil
}

func removePublishedAddress(path, address string) error {
	published, err := readPublishedAddress(path)
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return fmt.Errorf("streamnet: inspect endpoint publication before cleanup: %w", err)
	}
	if published != address {
		return fmt.Errorf("streamnet: endpoint publication changed before cleanup")
	}
	if err := os.Remove(path); err != nil && !errors.Is(err, os.ErrNotExist) {
		return fmt.Errorf("streamnet: remove endpoint publication: %w", err)
	}
	return nil
}
