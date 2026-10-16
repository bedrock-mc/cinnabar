"""Exercise update manifest publication offline with a fake release asset store."""

import os
import json
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
RELEASE_ASSETS = json.loads((ROOT / "packaging/release-assets.json").read_text())
CHANNELS = RELEASE_ASSETS["update_channels"]
VERSION_TAG = "v1.2.3"
PREFIX = RELEASE_ASSETS["update_payload_prefix"]
CHANGELOG_SPEC = importlib.util.spec_from_file_location("changelog", ROOT / "packaging/changelog.py")
changelog = importlib.util.module_from_spec(CHANGELOG_SPEC)
CHANGELOG_SPEC.loader.exec_module(changelog)
FAKE_GH = '''#!/bin/bash
set -eu
echo "$*" >> calls
case "$2" in
  list) : ;;
  view) [[ -d "releases/$3" ]] && { [[ "$*" != *--json* ]] || ls "releases/$3"; } ;;
  create)
    mkdir -p "releases/$3" states
    if [[ "$*" == *--draft* ]]; then echo draft > "states/$3";
    elif [[ "$*" == *--prerelease* ]]; then echo prerelease > "states/$3"; fi
    for file in "${@:4}"; do
      [[ "$file" != --* ]] || break
      [[ ! -f "$file" ]] || cp "$file" "releases/$3/"
    done
    ;;
  upload) for file in "${@:4}"; do [[ "$file" == --* ]] || cp "$file" "releases/$3/"; done ;;
  download)
    tag="$3"; shift 3
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --pattern) pattern="$2"; shift 2 ;;
        --dir) directory="$2"; shift 2 ;;
        *) shift ;;
      esac
    done
    mkdir -p "$directory"
    cp "releases/$tag/$pattern" "$directory/$pattern"
    ;;
  delete-asset) rm "releases/$3/$4" ;;
  edit)
    mkdir -p states bodies
    if [[ "$*" == *--latest=true* ]]; then echo "$3" > latest; echo published > "states/$3"; fi
    tag="$3"; shift 3
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --notes-file) cp "$2" "bodies/$tag"; shift 2 ;;
        *) shift ;;
      esac
    done
    ;;
  *) exit 43 ;;
esac
'''
FAKE_GIT = '''#!/bin/bash
[[ "$1" != rev-parse ]] || echo 0000000
'''
FAKE_CARGO = '''#!/bin/bash
set -eu
# Rust tests cover envelope parsing; this fixture models the publisher's retained name output.
IFS= read -r content < "${@: -1}" || [[ -n "$content" ]]
[[ "$content" == names* ]] || exit 1
printf '%s\\n' "${content#names }"
'''


def step(name):
    """Reads the release step's executable shell so assertions exercise publication behavior."""
    workflow = (ROOT / ".github/workflows/package.yml").read_text()
    block = workflow.split(f"      - name: {name}\n", 1)[1]
    script = block.split("        run: |\n", 1)[1].split("\n      - name: ", 1)[0]
    return "\n".join(line[10:] for line in script.splitlines())


class ManifestPublishTests(unittest.TestCase):
    def setUp(self):
        """Creates an isolated release store and command fixtures."""
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        notes = self.root / "release-tools/packaging/release-notes.sh"
        notes.parent.mkdir(parents=True)
        notes.write_text("exit 0\n")
        for name, script in [("gh", FAKE_GH), ("git", FAKE_GIT), ("cargo", FAKE_CARGO)]:
            (self.root / name).write_text(script)
            (self.root / name).chmod(0o755)
        (self.root / "dist").mkdir()

    def run_step(self, name, channel):
        """Runs a publication step against the fixture store, never a remote release."""
        release = CHANNELS[channel]
        env = dict(os.environ, PATH=f"{self.root}:{os.environ['PATH']}", RUNNER_TEMP=str(self.root),
                   GITHUB_WORKSPACE=str(self.root), GITHUB_OUTPUT=str(self.root / "step-output"), RELEASE_CHANNEL=channel,
                   RELEASE_TAG=self.holder(channel).name, RELEASE_TRACK=release["track"], RELEASE_COMMIT="0" * 40,
                   UPDATE_MANIFEST=release["manifest"], UPDATE_MANIFEST_TAG=self.holder(channel).name,
                   UPDATE_PAYLOAD_PREFIX=PREFIX)
        (self.root / "step-output").unlink(missing_ok=True)
        result = subprocess.run(["bash", "-eu", "-c", step(name)], cwd=self.root, env=env,
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result

    def holder(self, channel):
        """Returns a version tag for tagged releases and the fixed rolling prerelease tag."""
        release = CHANNELS[channel]
        tag = VERSION_TAG if release["track"] == "tagged" else release["tag"]
        return self.root / "releases" / tag

    def publish(self, channel, signed, generation="new", renewed=False):
        """Builds a new download and, when signed, a manifest naming a new payload."""
        (self.root / "dist/Cinnabar.dmg").write_text("new download")
        if signed:
            payloads = self.root / "update-payloads"
            payloads.mkdir(exist_ok=True)
            for payload in payloads.iterdir():
                payload.unlink()
            (payloads / f"{PREFIX}{generation}-Cinnabar.dmg").write_text(f"{generation} payload")
            renewal = "\nrenewed signature" if renewed else ""
            (self.root / CHANNELS[channel]["manifest"]).write_text(f"names {PREFIX}{generation}-Cinnabar.dmg{renewal}")
        else:
            (self.root / CHANNELS[channel]["manifest"]).unlink(missing_ok=True)
        if CHANNELS[channel]["track"] == "tagged":
            self.run_step("Prepare the tagged release", channel)
        result = self.run_step("Publish the signed update manifest", channel)
        if CHANNELS[channel]["track"] == "rolling":
            self.run_step("Publish the rolling prerelease", channel)
        else:
            self.run_step("Publish the tagged release", channel)
        return result

    def published_release(self, channel):
        """A channel whose current manifest names the payload beside it."""
        holder = self.holder(channel)
        holder.mkdir(parents=True)
        (holder / CHANNELS[channel]["manifest"]).write_text(f"names {PREFIX}old-Cinnabar.dmg")
        (holder / f"{PREFIX}old-Cinnabar.dmg").write_text("old payload")
        (holder / "Cinnabar.dmg").write_text("old download")
        return holder

    def files(self, directory):
        """Reads the retained fixture assets by filename."""
        return {path.name: path.read_text() for path in directory.iterdir()}

    def test_only_a_new_signed_manifest_sets_the_announcement_flag(self):
        """Unsigned and identical publications cannot trigger release notices."""
        self.publish("nightly", signed=False)
        self.assertFalse((self.root / "step-output").exists())
        self.publish("nightly", signed=True)
        # The following prerelease step has its own outputs, so run the manifest step directly too.
        (self.holder("nightly") / CHANNELS["nightly"]["manifest"]).unlink()
        self.run_step("Publish the signed update manifest", "nightly")
        self.assertEqual((self.root / "step-output").read_text().strip(), "published=true")
        self.run_step("Publish the signed update manifest", "nightly")
        self.assertFalse((self.root / "step-output").exists())

    def test_a_new_channel_holder_is_a_prerelease_that_never_becomes_latest(self):
        self.publish("nightly", signed=True)
        self.assertTrue((self.holder("nightly") / CHANNELS["nightly"]["manifest"]).is_file())
        create = next(line for line in (self.root / "calls").read_text().splitlines()
                      if line.startswith("release create"))
        self.assertIn("--prerelease --latest=false", create)
        self.assertFalse((self.root / "latest").exists())
        self.assertEqual((self.root / "states" / self.holder("nightly").name).read_text().strip(),
                         "prerelease")

    def test_signed_publication_retains_the_previous_manifest_payloads(self):
        for channel, release in CHANNELS.items():
            with self.subTest(channel=channel):
                self.setUp()
                holder = self.published_release(channel)
                self.publish(channel, signed=True)
                expected = {release["manifest"]: f"names {PREFIX}new-Cinnabar.dmg",
                            f"{PREFIX}new-Cinnabar.dmg": "new payload",
                            f"{PREFIX}old-Cinnabar.dmg": "old payload"}
                expected["Cinnabar.dmg"] = "new download"
                self.assertEqual(self.files(holder), expected)
                calls = (self.root / "calls").read_text().splitlines()
                payload_upload = next(index for index, call in enumerate(calls)
                                      if call.startswith("release upload") and f"{PREFIX}new-Cinnabar.dmg" in call)
                manifest_upload = next(index for index, call in enumerate(calls)
                                       if call.startswith("release upload") and release["manifest"] in call)
                self.assertLess(payload_upload, manifest_upload)
                self.assertFalse(any("delete-asset" in call and f"{PREFIX}old" in call for call in calls))

    def test_the_third_publication_deletes_only_payloads_two_publications_old(self):
        holder = self.published_release("beta")
        cached = (holder / CHANNELS["beta"]["manifest"]).read_text()
        self.publish("beta", signed=True)
        self.assertEqual((holder / cached.removeprefix("names ")).read_text(), "old payload")
        self.publish("beta", signed=True, generation="third")
        self.assertFalse((holder / f"{PREFIX}old-Cinnabar.dmg").exists())
        self.assertEqual((holder / f"{PREFIX}new-Cinnabar.dmg").read_text(), "new payload")
        self.assertEqual((holder / f"{PREFIX}third-Cinnabar.dmg").read_text(), "third payload")

    def test_retrying_an_identical_publication_keeps_the_grace_period_and_immutable_payloads(self):
        holder = self.published_release("beta")
        self.publish("beta", signed=True)
        (self.root / "calls").write_text("")
        self.publish("beta", signed=True, renewed=True)
        self.assertTrue((holder / f"{PREFIX}old-Cinnabar.dmg").exists())
        calls = (self.root / "calls").read_text().splitlines()
        self.assertFalse(any(call.startswith("release upload") and PREFIX in call for call in calls))

    def test_unreadable_previous_metadata_never_blocks_a_new_manifest_or_deletes_payloads(self):
        holder = self.published_release("beta")
        (holder / CHANNELS["beta"]["manifest"]).write_text("unreadable envelope\n")
        self.publish("beta", signed=True)
        self.assertTrue((holder / f"{PREFIX}old-Cinnabar.dmg").exists())
        self.assertEqual((holder / CHANNELS["beta"]["manifest"]).read_text(), f"names {PREFIX}new-Cinnabar.dmg")

    def test_beta_publishes_its_manifest_and_payload_on_the_latest_versioned_release(self):
        self.publish("beta", signed=True)
        self.assertEqual((self.root / "latest").read_text().strip(), VERSION_TAG)
        version = self.root / "releases" / VERSION_TAG
        manifest = CHANNELS["beta"]["manifest"]
        payload = (version / manifest).read_text().removeprefix("names ")
        self.assertEqual((version / payload).read_text(), "new payload")
        self.assertEqual({p.name for p in (self.root / "releases").iterdir()}, {VERSION_TAG})
        calls = (self.root / "calls").read_text().splitlines()
        create = next(call for call in calls if call.startswith("release create"))
        self.assertIn("--draft --latest=false --verify-tag", create)
        manifest_upload = next(index for index, call in enumerate(calls)
                               if call.startswith("release upload") and manifest in call)
        promotion = next(index for index, call in enumerate(calls) if "--latest=true" in call)
        self.assertLess(manifest_upload, promotion)

    def test_beta_reruns_keep_existing_payload_downloads_public(self):
        holder = self.published_release("beta")
        self.publish("beta", signed=True)
        calls = (self.root / "calls").read_text().splitlines()
        self.assertFalse(any("--draft=true" in call for call in calls))
        self.assertTrue((holder / f"{PREFIX}old-Cinnabar.dmg").is_file())

    def test_tagged_publication_keeps_generated_entries_and_install_notes_for_discord(self):
        holder = self.published_release("beta")
        (self.root / "bodies").mkdir()
        install = "## Installing\n\nDownload the installer.\n"
        pull_url = "https://github.com/o/r/pull/42"
        compare = f"https://github.com/o/r/compare/v1.2.2...{VERSION_TAG}"
        generated = f"## What's Changed\n\n* Add a feature by @contributor in {pull_url}\n\n**Full Changelog**: {compare}\n"
        body = self.root / "bodies" / holder.name
        body.write_text(install + "\n" + generated)
        (self.root / "notes.md").write_text(install)
        for _ in range(2):
            self.run_step("Publish the tagged release", "beta")
            published = body.read_text()
            self.assertEqual(published, install + "\n" + generated)
            entries = changelog.release_entries(published)
            self.assertEqual(entries, [{"number": "42", "title": "Add a feature", "url": pull_url}])
            message = changelog.payload(VERSION_TAG, "https://github.com/o/r/releases/latest", "",
                                        entries, {"42": "new"}, compare)
            self.assertIn("1 change", message["embeds"][0]["description"])

    def test_an_unsigned_new_beta_release_has_no_compatibility_manifest(self):
        self.publish("beta", signed=False)
        version = self.root / "releases" / VERSION_TAG
        self.assertEqual(self.files(version), {"Cinnabar.dmg": "new download"})
        self.assertEqual((self.root / "latest").read_text().strip(), VERSION_TAG)

    def test_without_signing_the_current_manifest_and_its_payloads_stay_consistent(self):
        for channel in CHANNELS:
            with self.subTest(channel=channel):
                self.setUp()
                holder = self.published_release(channel)
                result = self.publish(channel, signed=False)
                self.assertIn("::notice::", result.stdout)
                files = self.files(holder)
                named = files[CHANNELS[channel]["manifest"]].removeprefix("names ")
                self.assertEqual(files[named], "old payload")
                self.assertNotIn(f"{PREFIX}new-Cinnabar.dmg", files)
