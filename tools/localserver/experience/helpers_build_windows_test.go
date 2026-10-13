package experience

import (
	"context"
	"fmt"
	"os/exec"
	"strconv"
)

// configureHelperBuildCancellation terminates the build and its descendants by the owned PID.
func configureHelperBuildCancellation(cmd *exec.Cmd) {
	cmd.Cancel = func() error {
		ctx, cancel := context.WithTimeout(context.Background(), helperPipeWaitDelay)
		defer cancel()
		kill := exec.CommandContext(ctx, "taskkill", "/T", "/F", "/PID", strconv.Itoa(cmd.Process.Pid))
		kill.WaitDelay = helperPipeWaitDelay
		if out, err := kill.CombinedOutput(); err != nil {
			return fmt.Errorf("stopping build tree %d: %w\n%s", cmd.Process.Pid, err, out)
		}
		return nil
	}
}
