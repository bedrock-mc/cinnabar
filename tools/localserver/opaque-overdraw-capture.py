#!/usr/bin/env python3
"""Capture fixed local foliage scenes through the headless developer MCP only."""

import argparse
import json
import math
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
SCENES = {
    "grass-down": ([-15.5, 65.62, 0.5], 0, 90),
    "horizon": ([-15.5, 65.62, 0.5], 0, 0),
    "canopy": ([33.5, 75.62, 0.5], 0, 65),
    "cave": ([-15.5, 46.62, 0.5], 90, 0),
}


class Control:
    """Own the MCP subprocess without reading its private control endpoint."""

    def __init__(self, repo):
        """Start the checkout's stdio controller; it owns the client and local server."""
        suffix = ".exe" if sys.platform == "win32" else ""
        self.process = subprocess.Popen(
            [str(repo / f"target/debug/cinnabar-mcp{suffix}"), "--repo", str(repo)],
            cwd=repo, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True,
        )
        self.counter = 0

    def call(self, name, arguments):
        """Send one tool call without printing private transport responses."""
        self.counter += 1
        request = {
            "jsonrpc": "2.0", "id": self.counter, "method": "tools/call",
            "params": {"name": name, "arguments": arguments},
        }
        self.process.stdin.write(json.dumps(request) + "\n")
        self.process.stdin.flush()
        response = self.process.stdout.readline()
        if not response:
            raise RuntimeError(f"MCP closed while calling {name}")
        reply = json.loads(response)
        result = reply.get("result", {})
        if "error" in reply or result.get("isError"):
            raise RuntimeError(f"MCP {name} failed; inspect the local client log")
        return json.loads(result["content"][0]["text"])

    def close(self):
        """Stop only the client and local server started by this controller."""
        try:
            if self.process.poll() is None:
                self.call("quit", {})
        finally:
            self.process.stdin.close()
            self.process.wait(timeout=30)


def dimensions(value):
    """Accept positive physical pixel dimensions before starting any process."""
    if not re.fullmatch(r"[1-9][0-9]*x[1-9][0-9]*", value):
        raise argparse.ArgumentTypeError("size must be WIDTHxHEIGHT with positive integers")
    return [int(part) for part in value.split("x")]


def parse_args(argv=None):
    """Keep argument validation usable without launching a client."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--label", required=True, help="Build label for artifact names")
    parser.add_argument("--repo", type=Path, default=ROOT, help="Checkout containing built tools")
    parser.add_argument("--out", type=Path, help="Output directory; default: a fresh temporary directory")
    parser.add_argument("--binary", type=Path, help="Client executable override")
    parser.add_argument("--assets", type=Path, help="World carrier override; default: client asset discovery")
    parser.add_argument("--size", type=dimensions, default="1920x1080", help="Physical pixels, WIDTHxHEIGHT")
    diagnostics = parser.add_mutually_exclusive_group()
    diagnostics.add_argument("--categories", action="store_true", help="Separate category pass timing")
    diagnostics.add_argument("--overdraw", action="store_true", help="Layer counts only; omit performance summaries")
    parser.add_argument("--query-health", action="store_true", help="Retain bounded GPU timestamp validity counters")
    parser.add_argument("--seconds", type=int, default=20, help="Measured interval per scene")
    parser.add_argument("--warmup", type=int, default=15, help="Warmup after each camera change")
    parser.add_argument("--scene", choices=SCENES, help="Capture only this scene")
    args = parser.parse_args(argv)
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*", args.label):
        parser.error("label must contain only letters, digits, underscores, dots and hyphens")
    if args.seconds <= 0 or args.warmup < 0:
        parser.error("seconds must be positive and warmup must be non-negative")
    return args


def query_counter(value):
    """Accept only the unsigned counter and timestamp domain emitted by the profiler."""
    if type(value) is not int or not 0 <= value <= (1 << 64) - 1:
        raise ValueError("invalid GPU query counter")
    return value


def query_health_record(line):
    """Extract only bounded numeric query diagnostics, excluding logger prefixes and extra fields."""
    payload = line.partition("RUST_MCBE_GPU_QUERY_HEALTH ")[2].lstrip()
    if not payload.startswith("{") or len(payload) > 65536:
        return None
    try:
        data, _ = json.JSONDecoder().raw_decode(payload)
        interval = data["interval_ms"]
        if type(interval) not in (int, float) or not math.isfinite(interval) or not 0 <= interval <= (1 << 64) - 1:
            return None
        result = {"interval_ms": interval}
        for field in ("frames", "ring_skips", "readbacks", "map_failures"):
            result[field] = query_counter(data[field])
        stages = data["stages"]
        if not isinstance(stages, dict) or len(stages) > 128:
            return None
        result["stages"] = {}
        for name, health in stages.items():
            if not re.fullmatch(r"gpu_[a-z0-9_]{1,64}", name):
                return None
            stage = {field: query_counter(health[field]) for field in (
                "attempts", "valid", "zero_begin", "zero_end", "reversed", "sentinel",
            )}
            pair = health["first_invalid"]
            if pair is not None and (not isinstance(pair, list) or len(pair) != 2):
                return None
            stage["first_invalid"] = [query_counter(value) for value in pair] if pair is not None else None
            result["stages"][name] = stage
        return result
    except (KeyError, TypeError, ValueError, OverflowError):
        return None


def parse_metrics(lines, overdraw=False, query_health=False):
    """Retain only numeric stage counters and explicit layer diagnostics from the log."""
    intervals, layers, health = [], [], []
    for line in lines:
        if query_health and (record := query_health_record(line)) is not None:
            health.append(record)
        if line.startswith("RUST_MCBE_OPAQUE_LAYERS {"):
            layers.append(json.loads(line.split(" ", 1)[1]))
        elif not overdraw and line.startswith("RUST_MCBE_STAGE_PROFILE "):
            record = {}
            for field in line.split()[1:]:
                label, value = field.split("=", 1)
                record[label] = [float(part) for part in value.split(",")] if "," in value else float(value)
            intervals.append(record)
    return intervals, layers, health


def performance_summary(intervals):
    """Report headless main-frame throughput and independently averaged GPU spans."""
    counts = sum(interval["main_frame"][0] for interval in intervals)
    duration = sum(interval["interval_ms"] for interval in intervals)
    stages = {}
    labels = {label for interval in intervals for label in interval}
    for label in sorted(labels):
        if label.startswith("gpu_") and label != "gpu_preparation":
            count = sum(interval.get(label, (0, 0, 0))[0] for interval in intervals)
            total = sum(interval.get(label, (0, 0, 0))[1] for interval in intervals)
            if count:
                stages[label] = round(total / count, 4)
    return {
        "headless_fps": round(counts * 1000 / duration, 2) if duration else None,
        "stages_ms": stages, "intervals": len(intervals),
    }


def main(argv=None):
    """Capture warm intervals separately from startup and camera changes."""
    args = parse_args(argv)
    repo = args.repo.resolve()
    out = args.out.resolve() if args.out else Path(tempfile.mkdtemp(prefix="cinnabar-opaque-overdraw-"))
    out.mkdir(parents=True, exist_ok=True)
    size = "x".join(map(str, args.size))
    key = f"{args.label}-{size}" + ("-categories" if args.categories else "") + ("-layers" if args.overdraw else "")
    control = Control(repo)
    results = {}
    try:
        settings = {"RUST_MCBE_STAGE_PROFILE": "1"}
        if args.categories:
            settings["RUST_MCBE_GPU_CATEGORIES"] = "1"
        if args.overdraw:
            settings["RUST_MCBE_OPAQUE_LAYERS"] = "1"
        if args.query_health:
            settings["RUST_MCBE_GPU_QUERY_HEALTH"] = "1"
        client_args = ["--no-vsync", "--render-mode", "vanilla"]
        if args.assets:
            client_args.extend(["--assets", str(args.assets.resolve())])
        options = {
            "headless": True, "size": args.size, "args": client_args,
            "env": settings, "timeout_ms": 120000,
        }
        if args.binary:
            options["binary"] = str(args.binary.resolve())
        launch = control.call("launch_client", options)
        log = Path(launch["log"])
        print(json.dumps({"capture": key, "out": str(out), "headless": True}), flush=True)
        suffix = ".exe" if sys.platform == "win32" else ""
        control.call("connect", {"local_server": {
            "binary": str(repo / f"target/opaque-overdraw-localserver{suffix}"),
            "world_dir": str(out / "world"),
            "args": ["-opaque-overdraw", "-game-mode", "creative", "-difficulty", "peaceful"],
        }})
        control.call("wait_for", {"condition": "in_world", "timeout_ms": 120000})
        control.call("input", {"press": ["F1"]})
        for name, (position, yaw, pitch) in SCENES.items():
            if args.scene and name != args.scene:
                continue
            control.call("chat", {"text": f"/tp {position[0]} {position[1] - 1.62} {position[2]}"})
            control.call("camera_path", {"keyframes": [{
                "t": 0, "position": position, "yaw": yaw, "pitch": pitch, "fov": 70,
            }]})
            time.sleep(args.warmup)
            state = control.call("state", {})
            control.call("screenshot", {"path": str(out / f"{key}-{name}.png"), "inline": False})
            offset = log.stat().st_size
            start = time.monotonic()
            time.sleep(args.seconds)
            with log.open() as stream:
                stream.seek(offset)
                intervals, layers, health = parse_metrics(stream, args.overdraw, args.query_health)
            summary = {field: state.get(field) for field in ["feet", "eye", "chunks", "camera", "in_world"]}
            results[name] = {
                "state": summary, "elapsed_seconds": time.monotonic() - start,
                "intervals": intervals, "layers": layers, "query_health": health,
                "timing_eligible": not args.overdraw,
            }
            (out / f"{key}.json").write_text(json.dumps(results, indent=2))
            if args.overdraw and not layers:
                raise RuntimeError(
                    f"{name}: no layer samples; increase --seconds or inspect {log} "
                    "for unsupported draw modes or readback failures"
                )
            report = {"scene": name}
            if not args.overdraw:
                report.update(performance_summary(intervals))
            if layers:
                report["layer_probe"] = layers[-1]
            if args.query_health:
                report["query_health_intervals"] = len(health)
            print(json.dumps(report), flush=True)
    finally:
        control.close()


if __name__ == "__main__":
    main()
