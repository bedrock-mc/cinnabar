#!/usr/bin/env python3
"""Best-effort guard against pattern kills that also match other agents' processes."""
import json
import os
import re
import sys

BLOCKED = {"pkill", "killall"}
KEYWORDS = {"if", "then", "else", "elif", "do", "while", "until", "!", "{"}
# Options whose values must be skipped before looking for the wrapped command.
WRAPPERS = {
    "exec": {"-a"},
    "command": set(),
    "builtin": set(),
    "time": set(),
    "sudo": {"-u", "-g", "-h", "-p", "-C", "-D", "-r", "-t", "-U", "--user", "--group",
             "--host", "--prompt", "--close-from", "--chdir", "--role", "--type", "--other-user"},
    "env": {"-u", "-C", "--unset", "--chdir"},
    "nice": {"-n", "--adjustment"},
    "nohup": set(),
    "xargs": {"-I", "-L", "-n", "-P", "-s", "-d", "-E", "--max-lines", "--max-args",
              "--max-procs", "--max-chars", "--delimiter", "--eof"},
    "timeout": {"-k", "-s", "--kill-after", "--signal"},
    "setsid": set(),
    "caffeinate": {"-t", "-w"},
    "stdbuf": {"-i", "-o", "-e", "--input", "--output", "--error"},
}
ASSIGNMENT = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*=")
REDIRECT = re.compile(r"(?:&>>|&>|<<<|<<-|<<|>>|<>|>&|<&|>\||[<>])")


def commands(script, data=False):
    """Reads simple commands and substitutions, treating heredoc bodies as data."""
    words, word, segments, heredocs, stack = [], [], [], [], []
    i, quote, redirect, parens = 0, None, None, 0
    started = quoted = False

    def end_word():
        """Discards redirection targets and remembers heredoc delimiters."""
        nonlocal started, quoted, redirect
        if started:
            value = "".join(word)
            if redirect in {"<<", "<<-"}:
                heredocs.append((value, quoted, redirect == "<<-", words))
            elif redirect is None:
                words.append(value)
            word.clear()
            started = quoted = False
            redirect = None

    def end_command():
        """Saves the current command without its redirections."""
        nonlocal words
        end_word()
        if words:
            segments.append((words, None))
            words = []

    while i < len(script):
        c = script[i]
        body = data and not stack
        substitution = (c == "$" and script[i + 1:i + 2] == "(" and script[i + 2:i + 3] != "(")
        process_substitution = quote is None and not body and c in "<>" and script[i + 1:i + 2] == "("
        if quote == "'":
            if c == "'":
                quote = None
            else:
                word.append(c)
        elif c == "\\" and i + 1 < len(script):
            following = script[i + 1]
            if following == "\n":
                i += 2
                continue
            if (quote == '"' or body) and following not in '\\$`"':
                word.append(c)
            else:
                word.append(following)
                quoted = True
                i += 1
            started = True
        elif c == "#" and quote is None and not body and not started:
            # Comments end at a newline, even when they contain unmatched quotes.
            newline = script.find("\n", i)
            i = len(script) if newline < 0 else newline
            continue
        elif substitution or process_substitution or c == "`":
            if c == "`" and stack and stack[-1][-1] == "`" and quote is None:
                end_command()
                words, word, quote, started, quoted, redirect, parens, _ = stack.pop()
            else:
                stack.append((words, word, quote, True, quoted, redirect, parens, "`" if c == "`" else ")"))
                words, word, quote, started, quoted, redirect = [], [], None, False, False, None
                parens = 0
                if c != "`":
                    i += 1
        elif body:
            pass  # Only substitutions in an unquoted heredoc execute commands.
        elif quote == '"':
            if c == '"':
                quote = None
            else:
                word.append(c)
        elif c in "'\"":
            quote = c
            started = quoted = True
        elif c == ")" and stack and stack[-1][-1] == ")" and parens == 0:
            end_command()
            words, word, quote, started, quoted, redirect, parens, _ = stack.pop()
        elif c in "<>" or (c == "&" and script[i + 1:i + 2] == ">"):
            if started and "".join(word).isdigit():
                word.clear()  # An adjacent number is the redirection's file descriptor.
                started = False
            end_word()
            match = REDIRECT.match(script, i)
            redirect = match.group()
            i = match.end() - 1
        elif c in ";&|\n()":
            end_command()
            if stack and c in "()":
                parens += 1 if c == "(" else -1
            if c == "\n" and heredocs:
                i += 1
                for delimiter, literal, strip_tabs, consumer in heredocs:
                    body_lines = []
                    while i < len(script):
                        end = script.find("\n", i)
                        end = len(script) if end < 0 else end + 1
                        line = script[i:end]
                        i = end
                        if (line.lstrip("\t") if strip_tabs else line).rstrip("\n") == delimiter:
                            break
                        body_lines.append(line)
                    content = "".join(body_lines)
                    segments.append((consumer, content))
                    if not literal:
                        segments.extend(commands(content, data=True))
                heredocs.clear()
                continue
        elif c.isspace():
            end_word()
        else:
            word.append(c)
            started = True
        i += 1
    if quote is not None or stack:
        raise ValueError("unfinished shell quoting or substitution")
    if not data:
        end_command()
    return segments


def command_blocked(words, depth, stdin_script=None):
    """Checks the executable and common launchers that interpret command strings."""
    i = 0
    while i < len(words):
        w = words[i]
        name = os.path.basename(w)
        if w in KEYWORDS or ASSIGNMENT.match(w):
            i += 1
        elif name in BLOCKED:
            return True
        elif name == "eval":
            return scan(" ".join(words[i + 1:]), depth + 1)
        elif name in {"bash", "sh", "zsh"}:
            i += 1
            reads_stdin = False
            while i < len(words) and words[i].startswith("-") and words[i] != "--":
                option = words[i]
                i += 1
                if not option.startswith("--") and "c" in option[1:]:
                    if i < len(words) and words[i] == "--":
                        i += 1
                    return i < len(words) and scan(words[i], depth + 1)
                if option in {"-o", "-O"}:
                    i += 1
                elif not option.startswith("--") and "s" in option[1:]:
                    reads_stdin = True
            if i < len(words) and words[i] == "--":
                i += 1
            return (stdin_script is not None and (reads_stdin or i == len(words)) and
                    scan(stdin_script, depth + 1))
        elif name in WRAPPERS:
            takes = WRAPPERS[name]
            i += 1
            while i < len(words):
                option = words[i]
                if name == "command" and option in {"-v", "-V"}:
                    return False  # These options only report a command's name.
                if option == "--":
                    i += 1
                    break
                if name == "env" and (option in {"-S", "--split-string"} or
                                      option.startswith(("-S", "--split-string="))):
                    if option in {"-S", "--split-string"}:
                        i += 1
                        value = words[i] if i < len(words) else ""
                    else:
                        value = option.split("=", 1)[1] if option.startswith("--") else option[2:]
                    return scan(value + " " + " ".join(words[i + 1:]), depth + 1)
                if option in takes:
                    i += 2
                elif option.startswith("-") or (name == "env" and ASSIGNMENT.match(option)):
                    # Attached short values and long options with '=' occupy one word.
                    i += 1
                else:
                    break
            if name == "timeout":
                i += 1  # duration
        else:
            return False
    return False


def scan(script, depth=0):
    """Bounds launcher recursion so malformed or deeply nested input fails open."""
    if depth > 20:
        raise ValueError("command strings nested too deeply")
    return any(command_blocked(words, depth, stdin) for words, stdin in commands(script))


def blocked(script):
    """Returns false on parse errors; this cooperative guard is not a sandbox."""
    try:
        return scan(script)
    except (ValueError, TypeError, RecursionError):
        return False


if __name__ == "__main__":
    try:
        cmd = json.load(sys.stdin).get("tool_input", {}).get("command", "")
        deny = blocked(cmd)
    except Exception:
        sys.exit(0)  # fail open on malformed input
    if deny:
        print("Blocked: pkill/killall match other agents' and builds' command lines. "
              "Stop processes you started by PID (kill <pid>).", file=sys.stderr)
        sys.exit(2)
