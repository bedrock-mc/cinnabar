"""Checks the acceptance harness's local UDP reservations without starting BDS."""
import pathlib
import re
import socket
import subprocess
import sys
import unittest


class Reservations(unittest.TestCase):
    def test_ipv6_reservation_excludes_an_ipv6_only_listener(self):
        source = pathlib.Path(__file__).resolve().parents[2].joinpath("acceptance.sh").read_text()
        code = re.search(r"python3 -u -c '(import socket, sys\n.*?)' <\"\$control_path\"", source, re.S).group(1)
        process = subprocess.Popen([sys.executable, "-u", "-c", code], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        try:
            _, port = map(int, process.stdout.readline().split())
            with socket.socket(socket.AF_INET6, socket.SOCK_DGRAM) as competing:
                competing.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 1)
                with self.assertRaises(OSError):
                    competing.bind(("::", port))
        finally:
            process.stdin.close()
            process.wait(timeout=5)
            process.stdout.close()


if __name__ == "__main__":
    unittest.main()
