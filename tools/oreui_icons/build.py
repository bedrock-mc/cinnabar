"""Writes the shipped OreUI art and its source manifest from the drawings in this directory.

    python3 tools/oreui_icons/build.py           # regenerate assets/oreui-icons*
    python3 tools/oreui_icons/build.py --check   # fail if committed output differs
"""

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

import blocks
import controls
import glyphs
import scenes
import sleep
import tiles

ROOT = Path(__file__).resolve().parents[2]
KEYS_SOURCE = ROOT / "crates/oreui/src/images.rs"
ART_DIR = "oreui-icons"
MANIFEST = ROOT / "assets/oreui-icons-source.json"
GIF_FRAME_MS = 100
SLEEP_FRAME_MS = 500

# The install names its sleep animations by content hash; screens match them by prefix.
SLEEP_KEYS = {
    "steve": "assets/sleep_steve-5b5b685f51e1ff074190.gif",
    "alex": "assets/sleep_alex-fc863dc2c2fab9e7bc28.gif",
    "apple": "assets/sleep_dream_apple-d5fc732fee97c54947d0.gif",
    "creeper": "assets/sleep_dream_creeper-40220c8f629d9c72a3d1.gif",
    "diamond": "assets/sleep_dream_diamond-638b3a14282031c9db66.gif",
    "dirthut": "assets/sleep_dream_dirthut-88c2fdbcf83a4a1b272a.gif",
    "pickaxe": "assets/sleep_dream_pickaxe-52045d79bd091ad561e7.gif",
    "poppy": "assets/sleep_dream_poppy-30c798bd4883d33e12b0.gif",
}
SLEEP_STILLS = {
    "steve": "assets/sleep_steve_still-2a5f77b5a8a28ee33767.png",
    "alex": "assets/sleep_alex_still-03a8457d32eafa1b1b05.png",
    "apple": "assets/sleep_dream_apple_still-326c4b99ab6651cfb463.png",
    "creeper": "assets/sleep_dream_creeper_still-3769b288b3a9ec33dd2c.png",
    "diamond": "assets/sleep_dream_diamond_still-2565c29be34f7c4e711c.png",
    "dirthut": "assets/sleep_dream_dirthut_still-b48a2d94356013a9e4dd.png",
    "pickaxe": "assets/sleep_dream_pickaxe_still-6613aa8d49affe4d26a8.png",
    "poppy": "assets/sleep_dream_poppy_still-3ca37e5ab78ed5b85b98.png",
}

# Rust key constant -> drawings, in the constant's element order.
ART = {
    "WORLD_CATEGORY_ICONS": tiles.WORLD_CATEGORIES,
    "HARDCORE_ICON": [glyphs.HARDCORE],
    "LOADING_ANIMATION": [controls.LOADING],
    "SWITCH_ON_IMAGE": [controls.SWITCH_ON],
    "SWITCH_OFF_IMAGE": [controls.SWITCH_OFF],
    "CHEVRON_LEFT_IMAGE": [controls.CHEVRON_LEFT],
    "CHEVRON_UP_IMAGE": [controls.CHEVRON_UP],
    "CHEVRON_DOWN_IMAGE": [controls.CHEVRON_DOWN],
    "BASE_PACK_IMAGE": [blocks.BASE_PACK],
    "MISSING_PACK_IMAGE": [blocks.MISSING_PACK],
    "OVERWORLD_BLOCK_IMAGE": [blocks.OVERWORLD],
    "SETTINGS_ICON_HIGHLIGHT_IMAGE": [controls.GLIMMER],
    "SERVER_PING_IMAGES": glyphs.PINGS + [glyphs.ping_strip()],
    "SERVER_PLAYERS_IMAGE": [glyphs.PLAYERS],
    "SERVER_ADD_IMAGE": [controls.PLUS],
    "PLAY_TAB_ICONS": tiles.PLAY_TABS,
    "INBOX_ICONS": tiles.INBOX,
    "INBOX_EMPTY_IMAGES": scenes.INBOX_EMPTY,
    "SETTINGS_ICONS": tiles.SETTINGS,
    "PROFILE_STAT_ICONS": glyphs.PROFILE_STATS,
    "PROFILE_ERRORS": scenes.PROFILE_ERRORS,
    "PROFILE_SUMMARY_ICONS": glyphs.PROFILE_SUMMARY,
    "PROFILE_GAMERSCORE": [glyphs.SCORE],
    "ACTION_ICONS": controls.ACTIONS,
    "BUTTON_DISABLED_IMAGE": [controls.DISABLED],
    "BUTTON_PRIMARY_IMAGES": controls.PRIMARY,
    "BUTTON_SECONDARY_IMAGES": controls.SECONDARY,
    "BUTTON_NEUTRAL_IMAGES": controls.NEUTRAL,
    "BUTTON_DESTRUCTIVE_IMAGES": controls.DESTRUCTIVE,
    "PLAY_TAB_IMAGES": controls.TABS,
    "EXTERNAL_LINK_ICON": [controls.EXTERNAL],
    "RESET_ICON": [controls.RESET],
}


def rust_keys():
    """Reads each key constant's string literals from the Rust module that owns them."""
    source = KEYS_SOURCE.read_text()
    keys = {}
    pattern = re.compile(r"const (\w+): (?:&str|\[&str; \d+\]) =\s*(.*?);", re.S)
    for name, body in pattern.findall(source):
        keys[name] = re.findall(r'"(assets/[^"]+)"', body)
    return keys


def slug(key):
    stem = key.removeprefix("assets/").rsplit(".", 1)[0]
    return re.sub(r"[-.](?:[0-9a-f]{20}|[0-9a-f]{32})$", "", stem).replace("@", "-")


def entries():
    """(key, [(relative file, canvas, millis or None)]) for every shipped image."""
    keys = rust_keys()
    out = []
    for name, drawings in ART.items():
        if name not in keys or len(keys[name]) != len(drawings):
            raise SystemExit(f"{name}: expected {len(drawings)} keys in {KEYS_SOURCE.name}")
        group = name.lower().removesuffix("_images").removesuffix("_icons").removesuffix("_image")
        for key, drawing in zip(keys[name], drawings):
            base = f"{ART_DIR}/{group}/{slug(key)}"
            if isinstance(drawing, list):
                frames = [(f"{base}-{n:02}.png", f, GIF_FRAME_MS) for n, f in enumerate(drawing)]
            else:
                frames = [(f"{base}.png", drawing, None)]
            out.append((key, frames))
    for name, key in SLEEP_KEYS.items():
        drawn = sleep.frames(name)
        base = f"{ART_DIR}/sleep/{name}"
        out.append((key, [(f"{base}-{n:02}.png", f, SLEEP_FRAME_MS) for n, f in enumerate(drawn)]))
        out.append((SLEEP_STILLS[name], [(f"{base}-still.png", drawn[0], None)]))
    return out


def render():
    """Returns {relative path: bytes} for the manifest and every PNG."""
    files = {}
    images = []
    for key, frames in entries():
        listed = []
        for path, canvas, millis in frames:
            data = canvas.png()
            files[path] = data
            frame = {"file": path, "sha256": hashlib.sha256(data).hexdigest()}
            if millis is not None:
                frame["millis"] = millis
            listed.append(frame)
        images.append({"key": key, "frames": listed})
    manifest = {"schema": 1, "generator": "tools/oreui_icons/build.py", "images": images}
    files["oreui-icons-source.json"] = (json.dumps(manifest, indent=2) + "\n").encode()
    return files


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    files = render()
    assets = ROOT / "assets"
    art = assets / ART_DIR
    existing = {str(p.relative_to(assets)) for p in art.rglob("*.png")} if art.exists() else set()
    if args.check:
        stale = [p for p, data in files.items() if not (assets / p).is_file() or (assets / p).read_bytes() != data]
        stale += sorted(existing - set(files))
        if stale:
            sys.exit("out of date: " + ", ".join(stale[:8]))
        print(f"{len(files) - 1} images match")
        return
    for path in existing - set(files):
        (assets / path).unlink()
    for path, data in files.items():
        target = assets / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    print(f"wrote {len(files) - 1} images and {MANIFEST.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
