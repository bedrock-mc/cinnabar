package experience

import (
	"bufio"
	"errors"
	"fmt"
	"log/slog"
	"os"
	"os/exec"
	"reflect"
	"runtime"
	"slices"
	"sync"
	"sync/atomic"
	"time"
)

// errHelperFault is returned by Call when the helper missed the result deadline, wrote garbage
// or an oversized frame, answered another seq, or exited. The helper has been killed, reaped and,
// unless the fault quarantined the Experience, restarted; the fault counted as a strike.
var errHelperFault = errors.New("experience helper fault")

// errQuarantined is returned, without IPC, by every Call to a quarantined Experience.
var errQuarantined = errors.New("experience quarantined")

// errClosed is returned by Call and Reload after Close.
var errClosed = errors.New("experience supervisor closed")

// Supervisor runs the helper process of one Experience: it loads the artifact, runs callbacks
// one at a time, restarts the helper after a fault, and quarantines the Experience when strikes
// or restarts pile up. Every helper after the first must load the artifact that the first one
// loaded, whose blocks the server registered.
type Supervisor struct {
	binary, dir string
	opts        startOptions
	// registered is the first load. It is set before the Supervisor is shared and never changes.
	registered Loaded
	// log tags its records with the Experience id once the first load names it. The helpers'
	// stderr forwarders read it concurrently.
	log         atomic.Pointer[slog.Logger]
	quarantined atomic.Bool

	// mu serializes Call, Reload and Close, and guards the fields below.
	mu sync.Mutex
	// proc is nil while no helper runs: after a failed restart, in quarantine, and after Close.
	proc     *helper
	seq      uint64
	strikes  []time.Time
	restarts []time.Time
	closed   bool
}

// startOptions are test seams; production uses the zero value.
type startOptions struct {
	// env is added to the helper's otherwise cleared environment, to script the fake helper.
	env []string
	// now is the clock of the strike and restart windows; nil means time.Now.
	now func() time.Time
	// spawned, if set, is told of every helper process started, before its load.
	spawned func(*helper)
}

// StartSupervisor starts the helper binary, `experience-runtime`, with a cleared environment and
// private pipes, and loads the artifact in dir within loadDeadline. log receives the
// supervisor's records and the helper's stderr, tagged with the Experience id.
func StartSupervisor(binary, dir string, log *slog.Logger) (*Supervisor, Loaded, error) {
	return startSupervisor(binary, dir, log, startOptions{})
}

func startSupervisor(binary, dir string, log *slog.Logger, opts startOptions) (*Supervisor, Loaded, error) {
	if opts.now == nil {
		opts.now = time.Now
	}
	s := &Supervisor{binary: binary, dir: dir, opts: opts}
	s.log.Store(log.With("dir", dir))
	proc, loaded, err := s.spawn()
	if err != nil {
		return nil, Loaded{}, fmt.Errorf("starting the experience in %s: %w", dir, err)
	}
	s.registered = loaded
	s.log.Store(log.With("experience", loaded.ID))
	s.proc = proc
	return s, loaded, nil
}

// Call runs one callback and returns its outcome; it assigns req.Seq itself. Calls run one at a
// time, each within resultDeadline. A Failed outcome is a strike. A helper fault is a strike too:
// the helper is killed, reaped and restarted, and Call returns errHelperFault. A quarantined
// Experience returns errQuarantined without IPC.
func (s *Supervisor) Call(req CallbackRequest) (Outcome, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return Outcome{}, errClosed
	}
	if s.quarantined.Load() {
		return Outcome{}, errQuarantined
	}
	s.seq++
	req.Seq = s.seq
	body, err := encodeFrame(Request{Callback: &req})
	if err != nil {
		return Outcome{}, fmt.Errorf("encoding callback %d: %w", req.Seq, err)
	}
	if s.proc == nil {
		// The last restart failed; this call retries it.
		if err := s.restart(); err != nil {
			return Outcome{}, err
		}
	}
	resp, err := s.proc.roundTrip(body, resultDeadline)
	if err == nil {
		err = checkResult(resp, req.Seq)
	}
	if err != nil {
		s.fault(err)
		return Outcome{}, fmt.Errorf("%w: %v", errHelperFault, err)
	}
	outcome := resp.Result.Outcome
	if failed := outcome.Failed; failed != nil {
		s.logger().Warn("callback failed", "seq", req.Seq, "kind", failed.Kind, "reason", failed.Reason)
		s.strike()
	}
	return outcome, nil
}

// Focus reports whether the Experience's world takes its player's focus in client messages and
// epochs. Every helper's load must agree with the first.
func (s *Supervisor) Focus() bool {
	return s.registered.Focus
}

// Quarantined reports whether the Experience is quarantined. It never waits for a running Call.
func (s *Supervisor) Quarantined() bool {
	return s.quarantined.Load()
}

// Reload replaces the helper with a fresh one and clears the strikes, the restart history and
// any quarantine. The fresh helper must load the registered artifact (see respawn); when it does
// not, Reload returns why and leaves the Experience quarantined.
func (s *Supervisor) Reload() error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return errClosed
	}
	s.stopHelper()
	s.strikes, s.restarts = nil, nil
	proc, loaded, err := s.respawn()
	if err != nil {
		s.quarantine(fmt.Sprintf("reload failed: %v", err))
		return fmt.Errorf("reloading experience %s: %w", s.registered.ID, err)
	}
	s.proc = proc
	s.quarantined.Store(false)
	s.logger().Info("helper reloaded", "version", loaded.Version)
	return nil
}

// Close sends the helper the shutdown frame and kills it if it has not exited within
// shutdownGrace. It reports a helper that had to be killed or exited unsuccessfully.
func (s *Supervisor) Close() error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return nil
	}
	s.closed = true
	if s.proc == nil {
		return nil
	}
	err := s.proc.shutdown()
	s.proc = nil
	if err != nil {
		return fmt.Errorf("closing experience %s: %w", s.registered.ID, err)
	}
	return nil
}

func (s *Supervisor) logger() *slog.Logger {
	return s.log.Load()
}

// spawn starts a helper and loads the artifact, killing the helper again if the load fails.
func (s *Supervisor) spawn() (*helper, Loaded, error) {
	h, err := startHelper(s.binary, append(helperEnv(), s.opts.env...), &s.log)
	if err != nil {
		return nil, Loaded{}, err
	}
	if s.opts.spawned != nil {
		s.opts.spawned(h)
	}
	loaded, err := h.load(s.dir)
	if err != nil {
		h.kill()
		return nil, Loaded{}, err
	}
	return h, loaded, nil
}

// respawn spawns a helper that must load the registered artifact: the same id and the same
// blocks, in the same order. Only the version may change, so that a reload can ship fixed code. A
// helper whose load fails or differs is killed.
func (s *Supervisor) respawn() (*helper, Loaded, error) {
	h, loaded, err := s.spawn()
	if err != nil {
		return nil, Loaded{}, err
	}
	if err := matchRegistration(s.registered, loaded); err != nil {
		h.kill()
		return nil, Loaded{}, fmt.Errorf("the artifact in %s no longer matches its registration: %w", s.dir, err)
	}
	return h, loaded, nil
}

// fault handles a helper fault: the helper is killed and reaped, the fault counts as a strike,
// and unless that quarantines the Experience the helper restarts.
func (s *Supervisor) fault(cause error) {
	s.logger().Warn("helper fault", "err", cause)
	s.proc.kill()
	s.proc = nil
	if !s.strike() {
		// A failed restart is logged and counted inside.
		s.restart()
	}
}

// restart starts a fresh helper with the registered artifact (see respawn). The restart that
// makes more than maxRestarts within restartWindow quarantines the Experience instead. A failed
// or differing load is a fault: a strike, with no helper running until the next Call retries.
func (s *Supervisor) restart() error {
	now := s.opts.now()
	s.restarts = append(after(s.restarts, now.Add(-restartWindow)), now)
	if len(s.restarts) > maxRestarts {
		s.quarantine(fmt.Sprintf("%d restarts within %v", len(s.restarts), restartWindow))
		return errQuarantined
	}
	proc, loaded, err := s.respawn()
	if err != nil {
		s.logger().Warn("helper restart failed", "err", err)
		if s.strike() {
			return errQuarantined
		}
		return fmt.Errorf("%w: restarting the helper: %v", errHelperFault, err)
	}
	s.proc = proc
	s.logger().Info("helper restarted", "version", loaded.Version)
	return nil
}

// strike counts a strike and quarantines the Experience at strikeLimit strikes within
// strikeWindow. It reports whether the Experience is now quarantined.
func (s *Supervisor) strike() bool {
	now := s.opts.now()
	s.strikes = append(after(s.strikes, now.Add(-strikeWindow)), now)
	if len(s.strikes) < strikeLimit {
		return false
	}
	s.quarantine(fmt.Sprintf("%d strikes within %v", len(s.strikes), strikeWindow))
	return true
}

// quarantine disables the Experience until Reload: its helper stops and Call returns
// errQuarantined.
func (s *Supervisor) quarantine(reason string) {
	s.quarantined.Store(true)
	s.logger().Error("experience quarantined", "reason", reason)
	s.stopHelper()
}

// stopHelper shuts down the running helper, if any, logging an unclean end.
func (s *Supervisor) stopHelper() {
	if s.proc == nil {
		return
	}
	if err := s.proc.shutdown(); err != nil {
		s.logger().Warn("helper shutdown", "err", err)
	}
	s.proc = nil
}

// after drops the times at or before cutoff.
func after(times []time.Time, cutoff time.Time) []time.Time {
	return slices.DeleteFunc(times, func(t time.Time) bool { return !t.After(cutoff) })
}

// checkResult checks that resp answers callback seq with an outcome.
func checkResult(resp Response, seq uint64) error {
	switch result := resp.Result; {
	case result == nil:
		return errors.New("the answer to a callback is not a result")
	case result.Seq != seq:
		return fmt.Errorf("a result for seq %d answers callback %d", result.Seq, seq)
	case result.Outcome == (Outcome{}):
		return errors.New("a result without an outcome")
	}
	return nil
}

// matchRegistration checks that loaded has the id and exactly the blocks of registered, in the
// same order; the version may differ. The error names the first difference.
func matchRegistration(registered, loaded Loaded) error {
	if loaded.ID != registered.ID {
		return fmt.Errorf("its id changed from %q to %q", registered.ID, loaded.ID)
	}
	if loaded.Focus != registered.Focus {
		return fmt.Errorf("whether it takes a focus changed from %v to %v", registered.Focus, loaded.Focus)
	}
	if reflect.DeepEqual(loaded.Blocks, registered.Blocks) {
		return nil
	}
	ids := func(blocks []BlockDef) []string {
		out := make([]string, len(blocks))
		for i, block := range blocks {
			out[i] = block.ID
		}
		return out
	}
	if was, now := ids(registered.Blocks), ids(loaded.Blocks); !slices.Equal(was, now) {
		return fmt.Errorf("its blocks changed from %q to %q", was, now)
	}
	for i, was := range registered.Blocks {
		switch now := loaded.Blocks[i]; {
		case now.DisplayName != was.DisplayName:
			return fmt.Errorf("block %s: its display name changed from %q to %q", was.ID, was.DisplayName, now.DisplayName)
		case !slices.Equal(now.Textures, was.Textures):
			return fmt.Errorf("block %s: its textures changed from %v to %v", was.ID, was.Textures, now.Textures)
		case !reflect.DeepEqual(now.Mining, was.Mining):
			return fmt.Errorf("block %s: its mining changed from %s to %s", was.ID, miningText(was.Mining), miningText(now.Mining))
		}
	}
	return errors.New("its block definitions changed")
}

// miningText describes m for a message.
func miningText(m Mining) string {
	switch {
	case m.Unbreakable != nil:
		return "unbreakable"
	case m.Breakable != nil:
		return fmt.Sprintf("breakable with hardness %v", m.Breakable.Hardness)
	}
	return "none"
}

// helperEnv is the helper's environment: nothing but what the OS needs to start a process, which
// on Windows is SystemRoot. It is never nil, which would inherit the adapter's environment.
func helperEnv() []string {
	if runtime.GOOS == "windows" {
		return []string{"SystemRoot=" + os.Getenv("SystemRoot")}
	}
	return []string{}
}

// helper is one running helper process, connected by pipes that belong to it alone.
type helper struct {
	cmd    *exec.Cmd
	stdin  *os.File // the write end of the helper's stdin
	stdout *os.File // the read end of the helper's stdout
	// exited is closed once the process has exited and been reaped; err is then Wait's result.
	exited chan struct{}
	err    error
}

// startHelper starts `binary serve` with env as its whole environment and forwards its stderr,
// line by line, to the logger in log.
func startHelper(binary string, env []string, log *atomic.Pointer[slog.Logger]) (*helper, error) {
	var ends []*os.File // every pipe end opened here
	closeEnds := func() {
		for _, end := range ends {
			end.Close()
		}
	}
	pipe := func() (r, w *os.File, err error) {
		r, w, err = os.Pipe()
		if err == nil {
			ends = append(ends, r, w)
		}
		return r, w, err
	}
	stdinR, stdinW, err := pipe()
	if err != nil {
		return nil, err
	}
	stdoutR, stdoutW, err := pipe()
	if err != nil {
		closeEnds()
		return nil, err
	}
	stderrR, stderrW, err := pipe()
	if err != nil {
		closeEnds()
		return nil, err
	}
	cmd := exec.Command(binary, "serve")
	cmd.Env = env
	cmd.Stdin, cmd.Stdout, cmd.Stderr = stdinR, stdoutW, stderrW
	if err := cmd.Start(); err != nil {
		closeEnds()
		return nil, err
	}
	// The child holds its own copies of its ends; the parent's would hide its exit from readers.
	stdinR.Close()
	stdoutW.Close()
	stderrW.Close()
	h := &helper{cmd: cmd, stdin: stdinW, stdout: stdoutR, exited: make(chan struct{})}
	go func() {
		h.err = cmd.Wait()
		close(h.exited)
	}()
	go forwardStderr(stderrR, log)
	return h, nil
}

// forwardStderr logs each line that the helper writes to stderr until the helper closes it. A
// line longer than stderrLineBytes is logged cut short.
func forwardStderr(stderr *os.File, log *atomic.Pointer[slog.Logger]) {
	defer stderr.Close()
	lines := bufio.NewReaderSize(stderr, stderrLineBytes)
	rest := false // whether the line in progress was already logged, cut short
	for {
		line, more, err := lines.ReadLine()
		if err != nil {
			return
		}
		if !rest {
			if more {
				log.Load().Info("helper stderr", "line", string(line), "cut", true)
			} else {
				log.Load().Info("helper stderr", "line", string(line))
			}
		}
		rest = more
	}
}

// load sends the load request and waits loadDeadline for loaded, which must speak
// protocolVersion.
func (h *helper) load(dir string) (Loaded, error) {
	body, err := encodeFrame(Request{Load: &LoadRequest{Dir: dir, Items: serverItems()}})
	if err != nil {
		return Loaded{}, err
	}
	resp, err := h.roundTrip(body, loadDeadline)
	switch {
	case err != nil:
		return Loaded{}, fmt.Errorf("load: %w", err)
	case resp.LoadFailed != nil:
		return Loaded{}, fmt.Errorf("load failed: %s", resp.LoadFailed.Reason)
	case resp.Loaded == nil:
		return Loaded{}, errors.New("load was not answered with loaded")
	case resp.Loaded.Protocol != protocolVersion:
		return Loaded{}, fmt.Errorf("the helper speaks protocol %d, the adapter %d", resp.Loaded.Protocol, protocolVersion)
	}
	return *resp.Loaded, nil
}

// roundTrip writes body as one frame and reads the answer, both within deadline. After an error
// the helper is unusable and the caller kills it, which ends the goroutines left blocked here.
func (h *helper) roundTrip(body []byte, deadline time.Duration) (Response, error) {
	written := make(chan error, 1)
	go func() { written <- writeFrame(h.stdin, body) }()
	type answer struct {
		resp Response
		err  error
	}
	answers := make(chan answer, 1)
	go func() {
		resp, err := readFrame(h.stdout)
		answers <- answer{resp, err}
	}()
	timer := time.NewTimer(deadline)
	defer timer.Stop()
	var resp *Response
	for wrote := false; !wrote || resp == nil; {
		select {
		case err := <-written:
			if err != nil {
				return Response{}, fmt.Errorf("writing a frame: %w", err)
			}
			wrote = true
		case a := <-answers:
			if a.err != nil {
				return Response{}, a.err
			}
			resp = &a.resp
		case <-timer.C:
			return Response{}, fmt.Errorf("no answer within %v", deadline)
		}
	}
	return *resp, nil
}

// kill kills the helper and waits until it is reaped.
func (h *helper) kill() {
	// Kill fails only for a process that already exited, which the wait below then reaps.
	h.cmd.Process.Kill()
	h.release()
}

// shutdown sends the shutdown frame and ends stdin, then kills the helper if it has not exited
// within shutdownGrace. It reports a helper that had to be killed or exited unsuccessfully.
func (h *helper) shutdown() error {
	go func() {
		// A helper that stopped reading blocks this write until it is killed.
		if body, err := encodeFrame(Request{Shutdown: &ShutdownRequest{}}); err == nil {
			writeFrame(h.stdin, body)
		}
		h.stdin.Close()
	}()
	timer := time.NewTimer(shutdownGrace)
	defer timer.Stop()
	select {
	case <-h.exited:
		h.release()
		if h.err != nil {
			return fmt.Errorf("the helper exited after shutdown with %w", h.err)
		}
		return nil
	case <-timer.C:
		h.kill()
		return fmt.Errorf("the helper did not exit within %v of shutdown and was killed", shutdownGrace)
	}
}

// release closes the parent's pipe ends: stdin at once, stdout once the process has been reaped.
func (h *helper) release() {
	h.stdin.Close()
	<-h.exited
	h.stdout.Close()
}
