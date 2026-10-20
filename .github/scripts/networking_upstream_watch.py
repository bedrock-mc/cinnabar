"""Report public networking reference changes without translating or modifying source."""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import tomllib
import urllib.error
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
API_ROOT = "https://api.github.com/"
MAX_RESPONSE_BYTES = 8 * 1024 * 1024
MAX_DISCUSSION_PAGES = 10
COMMIT_PREVIEW = 20


class WatchError(Exception):
    """A local source or remote observation could not be interpreted completely."""


class APIGetRedirect(urllib.request.HTTPRedirectHandler):
    """Follow finite GET redirects only within the exact HTTPS API origin."""

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if req.get_method() != "GET":
            raise WatchError("GitHub redirect cannot change the request method")
        for url in (req.full_url, newurl):
            target = urllib.parse.urlsplit(url)
            if target.scheme != "https" or target.netloc.lower() != "api.github.com" or target.fragment:
                raise WatchError("GitHub redirect left the permitted API origin")
        # urllib otherwise consumes a redirect body without a byte bound before opening its target.
        if fp is not None and len(fp.read(MAX_RESPONSE_BYTES + 1)) > MAX_RESPONSE_BYTES:
            raise WatchError("GitHub redirect exceeds the response bound")
        return super().redirect_request(req, fp, code, msg, headers, newurl)


class GitHub:
    """Bounded, cached GET-only observations; remote content is never executed."""

    def __init__(self, token):
        self.token = token
        self.cache = {}
        self.opener = urllib.request.build_opener(APIGetRedirect())

    def __call__(self, path):
        if path in self.cache:
            return self.cache[path]
        if not path.startswith("repos/") or ":" in path or "#" in path:
            raise WatchError("invalid API observation path")
        request = urllib.request.Request(API_ROOT + path, headers={
            "Accept": "application/vnd.github+json",
            "X-GitHub-Api-Version": "2022-11-28",
            "User-Agent": "cinnabar-networking-upstream-watch",
        })
        if self.token:
            request.add_header("Authorization", f"Bearer {self.token}")
        try:
            with self.opener.open(request, timeout=30) as response:
                body = response.read(MAX_RESPONSE_BYTES + 1)
        except urllib.error.HTTPError as error:
            raise WatchError(f"GitHub HTTP {error.code}; observation unavailable") from None
        except (OSError, urllib.error.URLError):
            raise WatchError("GitHub observation failed") from None
        if len(body) > MAX_RESPONSE_BYTES:
            raise WatchError("GitHub observation exceeds the response bound")
        try:
            result = json.loads(body)
        except (ValueError, UnicodeError):
            raise WatchError("GitHub observation is not valid JSON") from None
        self.cache[path] = result
        return result


def repository(value):
    """Accept a concrete GitHub owner/repository, never an arbitrary destination URL."""
    if not isinstance(value, str) or not re.fullmatch(r"[\w.-]+/[\w.-]+", value, re.ASCII):
        raise WatchError("invalid GitHub repository")
    return value


def sha(value):
    """Require a full immutable commit identity before comparing a captured ref."""
    if not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{40}", value):
        raise WatchError("GitHub observation lacks a full commit identity")
    return value


def go_pin(text, module):
    """Read the explicit pin, rejecting unsupported local replacements instead of guessing."""
    text = "\n".join(line.split("//", 1)[0] for line in text.splitlines())
    name = re.escape(module)
    required = re.findall(rf"(?m)^\s*(?:require\s+)?{name}\s+(v\S+)\s*$", text)
    if len(required) != 1:
        raise WatchError("watched Go module needs one explicit versioned pin")
    replaced = []
    for suffix in re.findall(rf"(?m)^\s*(?:replace\s+)?{name}(?=\s)([^\n]*=>[^\n]*)$", text):
        old, new = (part.split() for part in suffix.split("=>", 1))
        if len(old) > 1:
            raise WatchError("unsupported watched Go replacement")
        if old and old != required:
            continue
        if len(new) != 2 or not new[0].startswith("github.com/") or not new[1].startswith("v"):
            raise WatchError("watched Go replacement must have an explicit GitHub version")
        replaced.append(new)
    if len(replaced) > 1:
        raise WatchError("watched Go module has ambiguous replacements")
    path, version = replaced[0] if replaced else (module, required[0])
    parts = path.split("/")
    if len(parts) < 3 or parts[0] != "github.com":
        raise WatchError("watched Go pin must resolve to GitHub")
    if len(parts) > 3 and (len(parts) != 4 or not re.fullmatch(r"v\d+", parts[3])):
        raise WatchError("watched Go submodule needs an explicit supported reference")
    pseudo = re.search(r"-(?:0\.)?\d{14}-([0-9a-f]{12,40})(?:\+incompatible)?$", version)
    return repository("/".join(parts[1:3])), pseudo[1] if pseudo else version.removesuffix("+incompatible")


def cargo_pin(lock, url):
    """Read the actual lock identity once across all packages from the same Git source."""
    if not url.startswith("https://github.com/"):
        raise WatchError("watched Cargo source must resolve to GitHub")
    revisions = set()
    for package in lock["package"]:
        source = package.get("source", "")
        if source.startswith("git+"):
            parsed = urllib.parse.urlsplit(source.removeprefix("git+"))
            if urllib.parse.urlunsplit(parsed._replace(query="", fragment="")) == url:
                revisions.add(sha(parsed.fragment))
    if len(revisions) != 1:
        raise WatchError("watched Cargo source must have one locked Git identity")
    return repository(url.removeprefix("https://github.com/").removesuffix(".git")), revisions.pop()


def baseline(selector, manifest, root, api):
    """Derive dependency pins from their owners, with separate explicit reference checkpoints."""
    if set(selector) == {"go_module"}:
        repo, revision = go_pin((root / "core/go.mod").read_text(), selector["go_module"])
        source = "core/go.mod: " + selector["go_module"]
    elif set(selector) == {"cargo_git"}:
        repo, revision = cargo_pin(tomllib.loads((root / "Cargo.lock").read_text()), selector["cargo_git"])
        source = "Cargo.lock: " + selector["cargo_git"]
    elif set(selector) == {"reference"}:
        item = manifest["references"][selector["reference"]]
        repo, revision = repository(item["repository"]), sha(item["revision"])
        source = "reference checkpoint: " + selector["reference"]
    else:
        raise WatchError("unsupported networking baseline selector")
    resolved = api(f"repos/{repo}/commits/{urllib.parse.quote(revision, safe='')}")
    return {"repository": repo, "revision": sha(resolved["sha"]), "source": source}


def observe_ref(repo, ref, pin, api):
    """Compare resolved immutable SHAs so later branch movement cannot alter this observation."""
    head = api(f"repos/{repo}/commits/{urllib.parse.quote(ref, safe='')}")
    revision = sha(head["sha"])
    path = f"repos/{repo}/compare/{pin['revision']}...{revision}?per_page=100"
    comparison = api(path)
    status = comparison["status"]
    if status not in {"identical", "ahead", "behind", "diverged"}:
        raise WatchError("unrecognized GitHub comparison status")
    commits = comparison["commits"]
    return {
        "repository": repo, "ref": ref, "baseline": pin, "head": revision,
        "status": status, "ahead": comparison["ahead_by"], "behind": comparison["behind_by"],
        "total_commits": comparison["total_commits"],
        "compare_url": f"https://github.com/{repo}/compare/{pin['revision']}...{revision}",
        "commits": [{"sha": sha(item["sha"]), "title": item["commit"]["message"].partition("\n")[0]}
                    for item in commits[:COMMIT_PREVIEW]],
        "commit_preview_truncated": comparison["total_commits"] > min(len(commits), COMMIT_PREVIEW),
    }


def pages(path, api):
    """Read a bounded complete discussion, failing explicitly rather than dropping later feedback."""
    items = []
    for page in range(1, MAX_DISCUSSION_PAGES + 1):
        batch = api(f"{path}?per_page=100&page={page}")
        if not isinstance(batch, list):
            raise WatchError("invalid GitHub discussion page")
        items.extend(batch)
        if len(batch) < 100:
            return items
    raise WatchError("GitHub discussion exceeds the pagination bound")


def instant(value):
    """Parse API UTC observations instead of comparing differently formatted timestamps lexically."""
    if not isinstance(value, str):
        raise WatchError("invalid GitHub observation time")
    result = datetime.fromisoformat(value.replace("Z", "+00:00"))
    if result.tzinfo is None:
        raise WatchError("GitHub observation time lacks its offset")
    return result


def observe_pull(item, manifest, api):
    """Fingerprint feedback as data; a replacement mention is not applicability or approval."""
    repo, number = repository(item["repository"]), item["number"]
    if type(number) is not int or number <= 0:
        raise WatchError("invalid watched pull request")
    reference = manifest["references"][item["reference"]]
    if repository(reference["repository"]) != repo:
        raise WatchError("pull request replacement belongs to another repository")
    replacement = sha(reference["revision"])
    pull = api(f"repos/{repo}/pulls/{number}")
    observed = instant(item["observed_at"])
    threads = {}
    mentioned = False
    for kind, path in {
        "comments": f"issues/{number}/comments", "reviews": f"pulls/{number}/reviews",
        "inline_comments": f"pulls/{number}/comments", "timeline": f"issues/{number}/timeline",
    }.items():
        records = []
        for record in pages(f"repos/{repo}/{path}", api):
            body = record.get("body") or ""
            mentioned |= replacement in body
            timestamp = record.get("updated_at") or record.get("submitted_at") or record.get("created_at")
            records.append({
                "id": record.get("id"), "url": record.get("html_url"),
                "event": record.get("event"), "state": record.get("state"),
                "at": timestamp, "newer_than_checkpoint": bool(timestamp and instant(timestamp) > observed),
                "commit": record.get("commit_id"),
                "body_sha256": hashlib.sha256(body.encode()).hexdigest() if body else None,
            })
        threads[kind] = records
    return {
        "repository": repo, "number": number, "url": f"https://github.com/{repo}/pull/{number}",
        "state": pull["state"], "merged": pull["merged"], "updated_at": pull["updated_at"],
        "newer_than_checkpoint": instant(pull["updated_at"]) > observed,
        "body_sha256": hashlib.sha256((pull.get("body") or "").encode()).hexdigest(),
        "head": sha(pull["head"]["sha"]), "base": sha(pull["base"]["sha"]),
        "replacement_reference": replacement, "replacement_mentioned": mentioned,
        "observed_at": item["observed_at"], "threads": threads,
    }


def collect(manifest, root, api):
    """Retain independent source failures and observations without changing any reviewed pin."""
    if manifest["schema"] != 1:
        raise WatchError("unsupported networking watch schema")
    result = {"observed_at": datetime.now(timezone.utc).isoformat(), "watches": [], "pull_requests": []}
    for watch in manifest["watches"]:
        repo = repository(watch["repository"])
        for ref in watch["refs"]:
            try:
                pin = baseline(watch["baseline"], manifest, root, api)
                value = observe_ref(repo, ref, pin, api)
            except (WatchError, KeyError, TypeError, ValueError, OSError) as error:
                value = {"repository": repo, "ref": ref, "error": str(error) if isinstance(error, WatchError) else "invalid source observation"}
            result["watches"].append(value)
    for item in manifest["pull_requests"]:
        try:
            value = observe_pull(item, manifest, api)
        except (WatchError, KeyError, TypeError, ValueError) as error:
            value = {"repository": item["repository"], "number": item["number"], "error": str(error) if isinstance(error, WatchError) else "invalid discussion observation"}
        result["pull_requests"].append(value)
    return result


def summary(result):
    """Render only static policy text and JSON-quoted observations; feedback never becomes shell input."""
    lines = ["# Networking upstream observations", "",
             "Changes need manual source-body/applicability review, owning regressions and normal gates.",
             "This report neither translates nor approves source. Full identities and feedback fingerprints are in the artifact.", ""]
    for item in result["watches"]:
        values = {key: item[key] for key in ("repository", "ref", "status", "ahead", "behind", "error") if key in item}
        lines.append("    " + json.dumps(values))
    lines += ["", "Pull request observations:", ""]
    for item in result["pull_requests"]:
        values = {key: item[key] for key in ("repository", "number", "state", "merged", "newer_than_checkpoint", "replacement_mentioned", "error") if key in item}
        if "threads" in item:
            values["newer_feedback"] = sum(record["newer_than_checkpoint"] for rows in item["threads"].values() for record in rows)
        lines.append("    " + json.dumps(values))
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--summary", type=Path, required=True)
    args = parser.parse_args()
    try:
        manifest = json.loads((ROOT / ".github/networking-upstreams.json").read_text())
        result = collect(manifest, ROOT, GitHub(os.environ.get("GH_TOKEN")))
        args.output.write_text(json.dumps(result, indent=2) + "\n")
        args.summary.write_text(summary(result))
    except (OSError, ValueError, KeyError, TypeError, WatchError):
        print("networking upstream report could not be completed", file=sys.stderr)
        return 1
    return int(any("error" in row for key in ("watches", "pull_requests") for row in result[key]))


if __name__ == "__main__":
    sys.exit(main())
