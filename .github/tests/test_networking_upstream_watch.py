"""Exercise source ownership and remote observation boundaries without Git, network or credentials."""

import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import tomllib
import unittest
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "networking_upstream_watch", ROOT / ".github/scripts/networking_upstream_watch.py"
)
watch = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(watch)

REPO = "example/reference"
PIN = "a" * 40
HEAD = "b" * 40


def comparison(total=1):
    """Supply one captured comparison, including a legal empty commit subject."""
    return {"status": "ahead", "ahead_by": total, "behind_by": 0, "total_commits": total,
            "commits": [{"sha": HEAD, "commit": {"message": ""}}]}


class SourceTests(unittest.TestCase):
    def test_go_pin_follows_effective_replacement_and_preserves_tags(self):
        module = "github.com/example/reference/v2"
        text = f"""require (
            {module} v2.0.0 // indirect
        )
        replace (
            {module} v1.0.0 => ../unused-version
            {module} v2.0.0 => github.com/example/fork/v2 v2.0.1-0.20261018123456-012345abcdef
        )"""
        self.assertEqual(watch.go_pin(text, module), ("example/fork", "012345abcdef"))
        self.assertEqual(watch.go_pin(f"require {module} v2.0.0+incompatible", module),
                         (REPO, "v2.0.0"))
        self.assertEqual(watch.go_pin(f"require {module} v2.0.0\nreplace {module} => github.com/example/fork/v2 v2.1.0", module),
                         ("example/fork", "v2.1.0"))

    def test_go_pin_never_falls_back_past_a_local_or_ambiguous_replacement(self):
        module = "github.com/example/reference"
        for suffix in [f"replace {module} => ../local", f"replace {module} => github.com/example/fork",
                       f"replace (\n{module} => github.com/example/fork v1.0.0\n{module} => github.com/example/other v1.0.0\n)"]:
            with self.subTest(suffix=suffix), self.assertRaises(watch.WatchError):
                watch.go_pin(f"require {module} v1.0.0\n{suffix}", module)

    def test_cargo_lock_has_one_nominal_identity_across_packages(self):
        url = "https://github.com/example/reference"
        lock = {"package": [
            {"name": "one", "source": f"git+{url}?rev={PIN}#{PIN}"},
            {"name": "two", "source": f"git+{url}?branch=main#{PIN}"},
        ]}
        self.assertEqual(watch.cargo_pin(lock, url), (REPO, PIN))
        lock["package"][1]["source"] = f"git+{url}?rev={HEAD}#{HEAD}"
        with self.assertRaises(watch.WatchError):
            watch.cargo_pin(lock, url)

    def test_real_watch_baselines_remain_derived_from_current_owners(self):
        manifest = json.loads((ROOT / ".github/networking-upstreams.json").read_text())
        go_mod = (ROOT / "core/go.mod").read_text()
        lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
        for entry in manifest["watches"]:
            selector = entry["baseline"]
            if "go_module" in selector:
                repo, revision = watch.go_pin(go_mod, selector["go_module"])
            elif "cargo_git" in selector:
                repo, revision = watch.cargo_pin(lock, selector["cargo_git"])
            else:
                reference = manifest["references"][selector["reference"]]
                repo, revision = watch.repository(reference["repository"]), watch.sha(reference["revision"])
            self.assertEqual(watch.repository(repo), repo)
            self.assertTrue(revision)


class ObservationTests(unittest.TestCase):
    def test_ref_is_resolved_once_before_immutable_comparison(self):
        paths = []

        def api(path):
            paths.append(path)
            if path == f"repos/{REPO}/commits/lunar":
                return {"sha": HEAD}
            self.assertEqual(path, f"repos/{REPO}/compare/{PIN}...{HEAD}?per_page=100")
            return comparison(total=200)

        value = watch.observe_ref(REPO, "lunar", {"revision": PIN}, api)
        self.assertEqual(value["head"], HEAD)
        self.assertEqual(value["commits"], [{"sha": HEAD, "title": ""}])
        self.assertTrue(value["commit_preview_truncated"])
        self.assertEqual(len(paths), 2)

    def test_discussion_reads_later_pages_and_refuses_incomplete_observation(self):
        pages = []

        def api(path):
            pages.append(path)
            return [{}] * 100 if path.endswith("page=1") else [{"id": 101}]

        rows = watch.pages("repos/example/reference/issues/8/comments", api)
        self.assertEqual(len(rows), 101)
        self.assertEqual(rows[-1], {"id": 101})
        self.assertTrue(pages[-1].endswith("page=2"))
        with self.assertRaises(watch.WatchError):
            watch.pages("repos/example/reference/issues/8/comments", lambda _: [{}] * 100)

    def test_pull_feedback_is_fingerprinted_not_executed_or_copied_as_instructions(self):
        item = {"repository": REPO, "number": 8, "reference": "replacement",
                "observed_at": "2026-10-18T17:16:49Z"}
        manifest = {"references": {"replacement": {"repository": REPO, "revision": PIN}}}
        body = f"replacement {PIN}; $(do-not-run); ```untrusted```"

        def api(path):
            if path == f"repos/{REPO}/pulls/8":
                return {"state": "closed", "merged": False, "updated_at": "2026-10-19T00:00:00Z",
                        "head": {"sha": HEAD}, "base": {"sha": PIN}, "body": "edited description"}
            if "/issues/8/comments?" in path:
                return [{"id": 1, "body": body, "created_at": "2026-10-18T00:00:00Z",
                         "updated_at": "2026-10-19T00:00:00Z", "html_url": "https://github.com/example/reference/pull/8#issuecomment-1"}]
            if "/timeline?" in path:
                return [{"id": 2, "event": "head_ref_deleted", "created_at": item["observed_at"]}]
            return []

        value = watch.observe_pull(item, manifest, api)
        self.assertFalse(value["merged"])
        self.assertTrue(value["newer_than_checkpoint"])
        self.assertTrue(value["replacement_mentioned"])
        comment = value["threads"]["comments"][0]
        self.assertTrue(comment["newer_than_checkpoint"])
        self.assertEqual(comment["body_sha256"], hashlib.sha256(body.encode()).hexdigest())
        self.assertFalse(value["threads"]["timeline"][0]["newer_than_checkpoint"])
        self.assertNotIn(body, json.dumps(value))

    def test_missing_sources_are_reported_without_discarding_other_refs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "core").mkdir()
            (root / "core/go.mod").write_text("require github.com/example/reference v1.0.0\n")
            manifest = {"schema": 1, "pull_requests": [], "watches": [
                {"repository": REPO, "refs": ["missing", "lunar"],
                 "baseline": {"go_module": "github.com/example/reference"}}
            ]}

            def api(path):
                if path.endswith("/commits/missing"):
                    raise watch.WatchError("observation unavailable")
                if "/commits/" in path:
                    return {"sha": PIN if path.endswith("/v1.0.0") else HEAD}
                return comparison()

            value = watch.collect(manifest, root, api)
            self.assertIn("error", value["watches"][0])
            self.assertEqual(value["watches"][1]["status"], "ahead")

    def test_api_uses_bounded_cached_gets_on_one_origin(self):
        class Response(io.BytesIO):
            def read(self, size=-1):
                self.requested = size
                return super().read(size)

        response = Response(b'{"sha":"' + HEAD.encode() + b'"}')
        requests = []

        class Opener:
            def open(self, request, timeout):
                requests.append((request, timeout))
                return response

        api = watch.GitHub("synthetic-fixture-token")
        api.opener = Opener()
        path = f"repos/{REPO}/commits/main"
        self.assertEqual(api(path), {"sha": HEAD})
        self.assertEqual(api(path), {"sha": HEAD})
        self.assertEqual(len(requests), 1)
        request, timeout = requests[0]
        self.assertEqual(request.get_method(), "GET")
        self.assertEqual(request.full_url, watch.API_ROOT + path)
        self.assertEqual(timeout, 30)
        self.assertEqual(response.requested, watch.MAX_RESPONSE_BYTES + 1)
        with self.assertRaises(watch.WatchError):
            api("https://elsewhere.invalid/")

    def test_redirect_handles_canonical_watch_and_derived_legacy_baseline(self):
        manifest = json.loads((ROOT / ".github/networking-upstreams.json").read_text())
        entry = next(row for row in manifest["watches"]
                     if row["baseline"].get("go_module") == "github.com/sandertv/go-raknet")
        legacy, revision = watch.go_pin((ROOT / "core/go.mod").read_text(), entry["baseline"]["go_module"])
        legacy_url = f"{watch.API_ROOT}repos/{legacy}/commits/{revision}"
        pinned = PIN
        redirected = []

        class Opener:
            def open(self, request, timeout):
                if request.full_url == legacy_url:
                    request = watch.APIGetRedirect().redirect_request(
                        request, io.BytesIO(b"moved"), 301, "moved", {},
                        f"{watch.API_ROOT}repositories/7/commits/{revision}")
                    redirected.append(request)
                    value = {"sha": pinned}
                elif request.full_url == f"{watch.API_ROOT}repos/{entry['repository']}/commits/lunar":
                    value = {"sha": HEAD}
                elif request.full_url == f"{watch.API_ROOT}repos/{entry['repository']}/compare/{pinned}...{HEAD}?per_page=100":
                    value = comparison()
                else:
                    raise AssertionError("unexpected fixture API request")
                return io.BytesIO(json.dumps(value).encode())

        api = watch.GitHub("synthetic-fixture-token")
        api.opener = Opener()
        result = watch.collect({"schema": 1, "watches": [entry], "pull_requests": []}, ROOT, api)
        value = result["watches"][0]
        self.assertEqual(value["repository"], entry["repository"])
        self.assertEqual(value["baseline"]["repository"], legacy)
        self.assertEqual(value["baseline"]["revision"], pinned)
        self.assertEqual(value["head"], HEAD)
        self.assertEqual(redirected[0].get_method(), "GET")
        self.assertEqual(redirected[0].get_header("Authorization"), "Bearer synthetic-fixture-token")

    def test_redirect_rejects_other_origins_ports_userinfo_and_method_changes(self):
        request = urllib.request.Request(watch.API_ROOT + "repos/example/reference",
                                         headers={"Authorization": "Bearer synthetic-fixture-token"})
        handler = watch.APIGetRedirect()
        for target in ["https://elsewhere.invalid/", "http://api.github.com/repos/example/reference",
                       "https://api.github.com:443/repos/example/reference",
                       "https://user@api.github.com/repos/example/reference",
                       "https://api.github.com/repos/example/reference#fragment"]:
            with self.subTest(target=target), self.assertRaises(watch.WatchError):
                handler.redirect_request(request, None, 301, "moved", {}, target)
        for method in ["HEAD", "POST", "PUT"]:
            changed = urllib.request.Request(request.full_url, method=method)
            with self.subTest(method=method), self.assertRaises(watch.WatchError):
                handler.redirect_request(changed, None, 301, "moved", {}, request.full_url)

    def test_redirect_preserves_standard_loop_limits_and_bounds_redirect_bodies(self):
        handler = watch.APIGetRedirect()
        target = watch.API_ROOT + "repositories/7"
        for visited in [{target: handler.max_repeats},
                        {f"{watch.API_ROOT}repositories/{index}": 1
                         for index in range(handler.max_redirections)}]:
            request = urllib.request.Request(watch.API_ROOT + "repos/example/reference")
            request.redirect_dict = visited
            with self.assertRaises(urllib.error.HTTPError) as redirect_error:
                handler.http_error_302(request, io.BytesIO(b"moved"), 301, "moved", {"location": target})
            redirect_error.exception.close()
        with self.assertRaises(watch.WatchError):
            handler.redirect_request(request, io.BytesIO(b"x" * (watch.MAX_RESPONSE_BYTES + 1)),
                                     301, "moved", {}, target)


if __name__ == "__main__":
    unittest.main()
