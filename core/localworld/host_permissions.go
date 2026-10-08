package localworld

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"
	"unicode"
)

// GrantHost grants command access only to the local client joining this exact cheats-enabled BDS instance.
func (m *Manager) GrantHost(ctx context.Context, address, name string) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.state != StateRunning || m.inst == nil || m.inst.Address() != address || m.world.Backend != BackendBDS || !m.world.AllowCheats {
		return nil
	}
	grant, ok := m.inst.(interface {
		GrantHost(context.Context, string) error
	})
	if !ok {
		return fmt.Errorf("localworld: host command permissions unavailable")
	}
	grantCtx, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()
	return grant.GrantHost(grantCtx, name)
}

// hostOperatorCommand quotes one exact player name, refusing selectors and console framing characters.
func hostOperatorCommand(name string) (string, error) {
	if name == "" || name != strings.TrimSpace(name) || strings.ContainsAny(name, "\"\\@*") || strings.ContainsFunc(name, unicode.IsControl) {
		return "", fmt.Errorf("localworld: local host identity unavailable; command permissions not granted")
	}
	return "op \"" + name + "\"", nil
}

// resetBDSPermissions prevents a previous host's permission grant from carrying into another managed session.
func resetBDSPermissions(dir string) error {
	if err := os.WriteFile(filepath.Join(dir, "permissions.json"), []byte("[]\n"), 0o600); err != nil {
		return fmt.Errorf("localworld: reset host permissions: %w", err)
	}
	return nil
}

// GrantHost sends a validated host-only grant through the native server's console.
func (b *bdsInstance) GrantHost(_ context.Context, name string) error {
	command, err := hostOperatorCommand(name)
	if err != nil {
		return err
	}
	console, ok := b.Instance.(interface{ send(string) error })
	if !ok {
		return fmt.Errorf("localworld: host command console unavailable")
	}
	return console.send(command)
}

// GrantHost sends the same host-only grant through the container's console helper.
func (c *containerInstance) GrantHost(ctx context.Context, name string) error {
	command, err := hostOperatorCommand(name)
	if err != nil {
		return err
	}
	return c.runner.dockerCmd(ctx, "exec", c.name, "send-command", command).Run()
}
