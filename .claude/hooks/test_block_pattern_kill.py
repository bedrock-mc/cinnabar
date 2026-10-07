"""Pattern kills are blocked wherever the shell would run them; quoted mentions are not."""
import importlib.util
import pathlib
import json
import subprocess
import sys
import unittest

spec = importlib.util.spec_from_file_location(
    "guard", pathlib.Path(__file__).with_name("block-pattern-kill.py"))
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)


class BlockPatternKill(unittest.TestCase):
    def test_reviewed_launcher_forms(self):
        for cmd in ["bash <<< 'pkill x'", "sudo sh <<< 'killall x'",
                    "env -S 'bash -c' 'pkill x'", 'bash -n -c "$(pkill x)"']:
            self.assertTrue(guard.blocked(cmd), cmd)
        for cmd in [
            "cat <<< 'pkill x'", "bash -c 'echo ok' <<< 'pkill x'",
            "bash -n -c 'pkill x'", "bash -nc 'pkill x'",
            "bash -o noexec -c 'pkill x'", "bash -n <<'EOF'\npkill x\nEOF",
            "env -S 'echo hi; pkill x'", "env -S 'echo' 'hello; pkill x'",
            "env -S 'bash -c' 'echo hi; echo pkill'",
        ]:
            self.assertFalse(guard.blocked(cmd), cmd)

    def test_nested_shell_forms(self):
        for cmd in [
            'echo "$( (echo ok); pkill x)"', "cat <(pkill x)", "cat >(killall x)",
            "command -- pkill x", "time -p pkill x", "exec -a label pkill x",
            "bash -c -- 'pkill x'",
        ]:
            self.assertTrue(guard.blocked(cmd), cmd)
        for cmd in ["echo ok &>/tmp/out pkill x", "echo ok &>>/tmp/out pkill x",
                    "exec -a pkill echo ok", "command -v pkill"]:
            self.assertFalse(guard.blocked(cmd), cmd)

    def test_hook_exit_status_and_fail_open(self):
        for payload, status in [
            (json.dumps({"tool_input": {"command": "pkill x"}}), 2),
            (json.dumps({"tool_input": {"command": "echo pkill"}}), 0),
            (json.dumps({"tool_input": {"command": "echo 'unfinished"}}), 0),
            (json.dumps({"tool_input": {"command": None}}), 0),
            ("not JSON", 0),
        ]:
            result = subprocess.run([sys.executable, spec.origin], input=payload,
                                    text=True, capture_output=True)
            self.assertEqual(result.returncode, status, payload)

    def test_continuations_and_comments(self):
        for cmd in ["pki\\\nll x", "sudo \\\n pkill x", "echo ok # 'ignored\npkill x"]:
            self.assertTrue(guard.blocked(cmd), cmd)
        for cmd in ["# pkill x", "echo ok # `pkill x`", "echo ok # $(pkill x)"]:
            self.assertFalse(guard.blocked(cmd), cmd)

    def test_command_strings(self):
        for cmd in [
            "bash -c 'pkill x'", 'sh -lc "killall x"', "zsh -c 'sudo pkill x'",
            "env -S 'pkill x'", "env --split-string='killall x'",
            "eval pkill x", "eval 'pkill' 'x'", "sudo bash -lc 'killall x'",
            "bash -c 'sh -c \"pkill x\"'",
        ]:
            self.assertTrue(guard.blocked(cmd), cmd)
        for cmd in ["bash -c 'echo pkill'", "env -S 'echo killall'", "eval 'echo pkill'",
                    "bash -c 'echo ok' pkill", "sh script.sh pkill"]:
            self.assertFalse(guard.blocked(cmd), cmd)

    def test_wrapper_values(self):
        for cmd in [
            "sudo --user root pkill x", "sudo --user=root pkill x", "sudo -u root pkill x",
            "sudo -uroot pkill x", "timeout -k 1 5 pkill x",
            "timeout --signal=KILL 5 pkill x", "timeout --signal KILL 5 pkill x",
            "nice -n 5 pkill x", "nice -5 pkill x", "nice --adjustment=5 pkill x",
            "xargs -I{} pkill {}", "xargs --replace={} killall {}",
            "env --unset FOO pkill x", "env --unset=FOO pkill x",
        ]:
            self.assertTrue(guard.blocked(cmd), cmd)
        for cmd in ["sudo --user pkill echo ok", "xargs -Ipkill echo ok",
                    "timeout --signal=KILL 5 echo pkill", "nice -n 5 echo pkill"]:
            self.assertFalse(guard.blocked(cmd), cmd)

    def test_redirections(self):
        for cmd in [">/tmp/x pkill x", "2>&1 pkill x", "pkill x 2>&1",
                    "echo ok 2>&1; pkill x", "<input sudo pkill x", "echo >$(pkill x)"]:
            self.assertTrue(guard.blocked(cmd), cmd)
        for cmd in ["echo ok >pkill", "2>&1 echo pkill", "echo '>/tmp/x pkill x'",
                    "cat <<< 'pkill x'"]:
            self.assertFalse(guard.blocked(cmd), cmd)

    def test_heredocs(self):
        for cmd in [
            "cat <<'EOF'\npkill x\n$(killall x)\n`pkill x`\nEOF",
            'cat <<"EOF"\npkill x\n$(killall x)\nEOF',
            "cat <<EOF\npkill x\nEOF", "cat <<\\EOF\n$(pkill x)\nEOF",
            "cat <<EOF\n\\$(pkill x)\nEOF",
            "cat <<-EOF\n\tpkill x\n\tEOF",
            "bash script.sh <<'EOF'\npkill x\nEOF",
            "bash -c 'echo ok' <<'EOF'\npkill x\nEOF",
        ]:
            self.assertFalse(guard.blocked(cmd), cmd)
        for cmd in [
            "cat <<EOF\n$(pkill x)\nEOF", "cat <<EOF\n`pkill x`\nEOF",
            "cat <<EOF\n'$(pkill x)'\nEOF", "cat <<EOF\nEOF\npkill x",
            "cat <<'ONE' <<TWO\n$(pkill ignored)\nONE\n$(killall x)\nTWO",
            "bash <<'EOF'\npkill x\nEOF", 'sh <<"EOF"\nkillall x\nEOF',
            "sudo bash -s arg <<'EOF'\npkill x\nEOF", "bash -- <<'EOF'\npkill x\nEOF",
        ]:
            self.assertTrue(guard.blocked(cmd), cmd)

    def test_blocks_executed_kills(self):
        for cmd in [
            "pkill -f bedrock-client",
            "sleep 1; pkill cargo",
            "echo x && killall node",
            "ls | xargs killall",
            "sudo pkill foo",
            "sudo -n pkill -f cinnabar",
            "sudo -u root pkill x",
            "nice -n 5 pkill x",
            "env FOO=bar killall cinnabar",
            "FOO=1 pkill x",
            "if true; then pkill -f cinnabar; fi",
            "/usr/bin/pkill x",
            "timeout 5 pkill x",
            'echo "$(pkill x)"',
            'echo "`pkill x`"',
            "(pkill x)",
        ]:
            self.assertTrue(guard.blocked(cmd), cmd)

    def test_allows_mentions_and_other_commands(self):
        for cmd in [
            "kill 12345; grep pkill file",
            'git log --grep="(pkill and killall)"',
            "cat > b.md <<EOF\nnever use pkill or killall\nEOF",
            'echo "pkill is banned"',
            "echo 'run `pkill x` never'",
            "pgrep -fl codex",
            "gh pr create --body-file body.md",
        ]:
            self.assertFalse(guard.blocked(cmd), cmd)


if __name__ == "__main__":
    unittest.main()
