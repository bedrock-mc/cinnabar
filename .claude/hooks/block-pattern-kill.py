#!/usr/bin/env python3
"""Blocks pkill/killall run as commands; pattern kills also match other agents' and builds' command lines."""
import json
import os
import re
import sys

BLOCKED = {"pkill", "killall"}
KEYWORDS = {"if", "then", "else", "elif", "do", "while", "until", "!", "{", "time", "exec", "command", "builtin"}
# Wrappers whose next non-option word is the real command; values are options that take an argument.
WRAPPERS = {
    "sudo": {"-u", "-g", "-h", "-p", "-C", "-D", "-r", "-t", "-U"},
    "env": {"-u", "-C", "-S"},
    "nice": {"-n"},
    "nohup": set(),
    "xargs": {"-I", "-L", "-n", "-P", "-s", "-d", "-E"},
    "timeout": set(),
    "setsid": set(),
    "caffeinate": {"-t", "-w"},
    "stdbuf": {"-i", "-o", "-e"},
}
ASSIGNMENT = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*=")


def commands(script):
    """Yields the word list of every simple command, including inside $(...) and backticks."""
    words, word, segments = [], [], []
    i, quote = 0, None
    stack = []  # saved (words, word, quote) for nested substitutions

    def end_word():
        if word:
            words.append("".join(word))
            word.clear()

    def end_command():
        end_word()
        if words:
            segments.append(list(words))
            words.clear()

    while i < len(script):
        c = script[i]
        if quote == "'":
            if c == "'":
                quote = None
            else:
                word.append(c)
        elif c == "\\" and i + 1 < len(script):
            word.append(script[i + 1])
            i += 1
        elif c == "$" and script[i + 1 : i + 2] == "(":
            stack.append((list(words), list(word), quote, ")"))
            words.clear(), word.clear()
            quote = None
            i += 1
        elif c == "`":
            if stack and stack[-1][3] == "`":
                end_command()
                saved_words, saved_word, quote, _ = stack.pop()
                words[:], word[:] = saved_words, saved_word
            else:
                stack.append((list(words), list(word), quote, "`"))
                words.clear(), word.clear()
                quote = None
        elif quote == '"':
            if c == '"':
                quote = None
            else:
                word.append(c)
        elif c in "'\"":
            quote = c
        elif c == ")" and stack and stack[-1][3] == ")":
            end_command()
            saved_words, saved_word, quote, _ = stack.pop()
            words[:], word[:] = saved_words, saved_word
        elif c in ";&|\n(){}":
            end_command()
        elif c.isspace():
            end_word()
        else:
            word.append(c)
        i += 1
    end_command()
    return segments


def program(words):
    """Returns the executable a simple command runs, skipping keywords, assignments and wrappers."""
    i = 0
    while i < len(words):
        w = words[i]
        name = os.path.basename(w)
        if w in KEYWORDS or ASSIGNMENT.match(w):
            i += 1
        elif name in WRAPPERS:
            takes = WRAPPERS[name]
            i += 1
            while i < len(words) and (words[i].startswith("-") or ASSIGNMENT.match(words[i])):
                i += 2 if words[i] in takes else 1
            if name == "timeout" and i < len(words):
                i += 1  # duration
        else:
            return name
    return None


def blocked(script):
    return any(program(words) in BLOCKED for words in commands(script))


if __name__ == "__main__":
    try:
        cmd = json.load(sys.stdin).get("tool_input", {}).get("command", "")
    except Exception:
        sys.exit(0)  # fail open on malformed input
    if blocked(cmd):
        print("Blocked: pkill/killall match other agents' and builds' command lines. "
              "Stop processes you started by PID (kill <pid>).", file=sys.stderr)
        sys.exit(2)
