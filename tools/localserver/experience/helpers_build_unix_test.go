//go:build unix

package experience

import (
	"bufio"
	"errors"
	"io"
	"os"
	"os/exec"
	"syscall"
	"testing"
	"time"
)

// configureHelperBuildCancellation gives the build its own process group and kills that group.
func configureHelperBuildCancellation(cmd *exec.Cmd) {
	cmd.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
	cmd.Cancel = func() error {
		err := syscall.Kill(-cmd.Process.Pid, syscall.SIGKILL)
		if errors.Is(err, syscall.ESRCH) {
			return os.ErrProcessDone
		}
		return err
	}
}

func TestHelperBuildCancellationStopsChildren(t *testing.T) {
	for _, test := range []struct {
		name   string
		signal syscall.Signal
	}{
		{name: "context"},
		{name: "interrupt", signal: syscall.SIGINT},
		{name: "termination", signal: syscall.SIGTERM},
	} {
		t.Run(test.name, func(t *testing.T) {
			input, inputWriter, err := os.Pipe()
			if err != nil {
				t.Fatal(err)
			}
			defer input.Close()
			defer inputWriter.Close()
			output, outputWriter, err := os.Pipe()
			if err != nil {
				t.Fatal(err)
			}
			defer output.Close()
			defer outputWriter.Close()
			if err := output.SetReadDeadline(time.Now().Add(helperPipeWaitDelay)); err != nil {
				t.Fatal(err)
			}
			ctx, cancel := helperBuildContext(t.Context(), helperPipeWaitDelay)
			defer cancel()
			cmd := helperBuildCommand(ctx, "sh", "-c", "cat <&3 & printf 'ready\\n'; wait")
			cmd.ExtraFiles = []*os.File{input}
			cmd.Stdout = outputWriter
			if err := cmd.Start(); err != nil {
				t.Fatal(err)
			}
			waited := false
			defer func() {
				cancel()
				if !waited {
					cmd.Wait()
				}
			}()
			input.Close()
			outputWriter.Close()
			reader := bufio.NewReader(output)
			if line, err := reader.ReadString('\n'); err != nil || line != "ready\n" {
				t.Fatalf("waiting for child: %q, %v", line, err)
			}
			if test.signal == 0 {
				cancel()
			} else if err := syscall.Kill(os.Getpid(), test.signal); err != nil {
				t.Fatal(err)
			}
			err = cmd.Wait()
			waited = true
			if err == nil {
				t.Fatal("canceled build succeeded")
			}
			if _, err := reader.ReadByte(); !errors.Is(err, io.EOF) {
				t.Fatalf("child still holds the output pipe after cancellation: %v", err)
			}
		})
	}
}
