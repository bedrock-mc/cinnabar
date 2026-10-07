"""Pattern kills are blocked wherever the shell would run them; quoted mentions are not."""
import importlib.util
import pathlib
import unittest

spec = importlib.util.spec_from_file_location(
    "guard", pathlib.Path(__file__).with_name("block-pattern-kill.py"))
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)


class BlockPatternKill(unittest.TestCase):
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
